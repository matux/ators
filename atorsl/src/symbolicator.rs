use crate::{data::*, *};
use fallible_iterator::FallibleIterator;
use gimli::{
    ColumnType, DW_AT_abstract_origin, DW_AT_artificial, DW_AT_call_column, DW_AT_call_file,
    DW_AT_call_line, DW_AT_decl_column, DW_AT_decl_file, DW_AT_decl_line, DW_AT_high_pc,
    DW_AT_linkage_name, DW_AT_low_pc, DW_AT_name, DW_AT_ranges, DebugInfoOffset, LineRow,
    UnitSectionOffset,
};
use itertools::Either;
use object::Object;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

pub fn atos_dwarf(dwarf: &Dwarf, addr: Addr, include_inlined: bool) -> Result<Vec<Symbol>, Error> {
    let unit = dwarf.unit_from_addr(&addr)?;
    let mut entries = unit.entries();

    let comp_dir = PathBuf::from(
        &*unit
            .comp_dir
            .ok_or(Error::CompUnitDirMissing(addr))?
            .to_string_lossy(),
    );

    let mut debug_line_rows = unit
        .line_program
        .clone()
        .ok_or_else(|| Error::CompUnitLineProgramMissing(addr))?
        .rows();

    let mut symbols = Vec::default();

    let subprogram = loop {
        let (_, entry) = entries
            .next_dfs()?
            .ok_or_else(|| Error::AddrNotFound(addr))?;

        if matches!(
            entry.tag(),
            gimli::DW_TAG_subprogram if dwarf.entry_contains(entry, &addr, &unit)
        ) {
            break entry;
        }
    };

    if include_inlined && subprogram.has_children() {
        let mut parent = subprogram.clone();
        let mut depth = 0;

        let last_child = loop {
            let Some((step, child)) = entries.next_dfs()? else {
                break parent
            };

            depth += step;

            if depth <= 0 {
                break parent;
            }

            if matches!(
                child.tag(),
                gimli::DW_TAG_inlined_subroutine if dwarf.entry_contains(child, &addr, &unit)
            ) {
                symbols.insert(
                    0,
                    Symbol {
                        addr,
                        name: demangler::demangle(&dwarf.entry_symbol(addr, &parent, &unit)?),
                        loc: Either::Left(dwarf.entry_source_loc(child, &comp_dir, &unit)),
                    },
                );

                parent = child.clone();
            }
        };

        symbols.insert(
            0,
            Symbol {
                addr,
                name: demangler::demangle(&dwarf.entry_symbol(addr, &last_child, &unit)?),
                loc: Either::Left(Some(dwarf.entry_debug_line(
                    &addr,
                    &mut debug_line_rows,
                    &unit,
                )?)),
            },
        );
    } else {
        symbols.push(Symbol {
            addr,
            name: demangler::demangle(&dwarf.entry_symbol(addr, subprogram, &unit)?),
            loc: Either::Left(Some(dwarf.entry_debug_line(
                &addr,
                &mut debug_line_rows,
                &unit,
            )?)),
        });
    }

    Ok(symbols)
}

pub fn atos_obj(obj: &object::File, addr: Addr) -> Result<Vec<Symbol>, Error> {
    let map = obj.symbol_map();
    let Some(symbol) = map.get(*addr) else {
        Err(Error::AddrNotFound(addr))?
    };

    Ok(vec![Symbol {
        addr: Addr::from(symbol.address()),
        name: demangler::demangle(symbol.name()),
        loc: Either::Right(addr - symbol.address()),
    }])
}

trait DwarfExt {
    fn entry_name<'a>(&'a self, entry: &'a Entry, unit: &'a Unit) -> Result<Cow<str>, Error>;
    fn entry_symbol<'a>(
        &'a self,
        addr: Addr,
        entry: &'a Entry,
        unit: &'a Unit,
    ) -> Result<Cow<str>, Error>;

    fn entry_source_loc(&self, entry: &Entry, path: &Path, unit: &Unit) -> Option<SourceLoc>;
    fn entry_debug_line(
        &self,
        addr: &Addr,
        line_rows: &mut IncompleteLineProgramRows,
        unit: &Unit,
    ) -> Result<SourceLoc, Error>;

    fn entry_contains(&self, entry: &Entry, addr: &Addr, unit: &Unit) -> bool;
    fn entry_pc_contains(&self, entry: &Entry, addr: &Addr) -> Option<bool>;
    fn entry_ranges_contain(&self, entry: &Entry, addr: &Addr, unit: &Unit) -> Option<bool>;

    fn line_row_file(
        &self,
        row: &LineRow,
        header: &LineProgramHeader,
        unit: &Unit,
    ) -> Result<PathBuf, Error>;

    fn attr_lossy_string<'a>(
        &'a self,
        unit: &Unit<'a>,
        attr: AttrValue<'a>,
    ) -> Result<Cow<str>, gimli::Error>;

    fn unit_from_offset(&self, addr: Addr, offset: DebugInfoOffset) -> Result<Unit, Error>;
    fn unit_from_addr(&self, addr: &Addr) -> Result<Unit, Error>;

    fn debug_info_offset(&self, addr: &Addr) -> Result<DebugInfoOffset, Error>;
}

impl DwarfExt for Dwarf<'_> {
    fn entry_name<'a>(&'a self, entry: &'a Entry, unit: &'a Unit) -> Result<Cow<str>, Error> {
        Ok(match entry.attr_value(DW_AT_name)? {
            Some(AttrValue::UnitRef(offset)) => Cow::Owned(
                self.entry_name(&unit.entry(offset)?, unit)?
                    .into_owned(),
            ),
            Some(attr) => self.attr_lossy_string(unit, attr)?,
            None => Err(Error::AddrNameMissing)?,
        })
    }

    fn entry_symbol<'a>(
        &'a self,
        addr: Addr,
        entry: &'a Entry,
        unit: &'a Unit,
    ) -> Result<Cow<str>, Error> {
        [DW_AT_linkage_name, DW_AT_abstract_origin, DW_AT_name]
            .into_iter()
            .find_map(|dw_at| entry.attr_value(dw_at).ok()?)
            .ok_or(Error::AddrSymbolMissing(addr))
            .and_then(|attr| match attr {
                AttrValue::UnitRef(offset) => Ok(Cow::Owned(
                    self.entry_symbol(addr, &unit.entry(offset)?, unit)?
                        .into_owned(),
                )),

                AttrValue::DebugInfoRef(offset) => {
                    let new_unit = self.unit_from_offset(addr, offset)?;
                    let new_entry = new_unit.entry(
                        UnitSectionOffset::from(offset)
                            .to_unit_offset(&new_unit)
                            .ok_or(Error::AddrDebugInfoRefOffsetOutOfBounds(addr))?,
                    )?;

                    Ok(Cow::Owned(
                        self.entry_symbol(addr, &new_entry, &new_unit)?
                            .into_owned(),
                    ))
                }

                attr => Ok(self.attr_lossy_string(unit, attr)?),
            })
    }

    fn entry_debug_line(
        &self,
        addr: &Addr,
        line_rows: &mut IncompleteLineProgramRows,
        unit: &Unit,
    ) -> Result<SourceLoc, Error> {
        let mut file = None;
        let mut source_locs = Vec::default();

        while let Some((header, line_row)) = line_rows.next_row()? {
            if line_row.address() == addr {
                if file.is_none() {
                    file.replace(self.line_row_file(line_row, header, unit)?);
                }

                source_locs.push(SourceLoc {
                    // SAFETY: `file` is always `Some` at this point.
                    file: unsafe { file.clone().unwrap_unchecked() },
                    line: line_row
                        .line()
                        .map(|line| line.get() as u16)
                        .unwrap_or_default(),
                    col: match line_row.column() {
                        ColumnType::LeftEdge => 0,
                        ColumnType::Column(c) => c.get() as u16,
                    },
                });
            }
        }

        source_locs
            .pop()
            .ok_or(Error::AddrLineInfoMissing(*addr))
    }

    fn entry_source_loc(&self, entry: &Entry, path: &Path, unit: &Unit) -> Option<SourceLoc> {
        let Some(AttrValue::FileIndex(offset)) = [DW_AT_decl_file, DW_AT_call_file]
            .into_iter()
            .find_map(|name| entry.attr_value(name).ok()?)
        else {
            return Some(SourceLoc {
                file: path.join("<compiler-generated>"),
                line: 0,
                col: 0,
            })
        };

        let header = unit.line_program.as_ref()?.header();
        let file = header.file(offset)?;
        let is_artificial = entry.attr_value(DW_AT_artificial) == Ok(Some(AttrValue::Flag(true)));

        Some(SourceLoc {
            file: PathBuf::from(
                &*self
                    .attr_lossy_string(unit, file.directory(header)?)
                    .ok()?,
            )
            .join(&*self.attr_lossy_string(unit, file.path_name()).ok()?),

            line: if is_artificial {
                0
            } else {
                [DW_AT_decl_line, DW_AT_call_line]
                    .into_iter()
                    .find_map(|name| entry.attr_value(name).ok()??.u16_value())?
            },

            col: if is_artificial {
                0
            } else {
                [DW_AT_decl_column, DW_AT_call_column]
                    .into_iter()
                    .find_map(|name| entry.attr_value(name).ok()??.u16_value())
                    .unwrap_or_default()
            },
        })
    }

    fn entry_contains(&self, entry: &Entry, addr: &Addr, unit: &Unit) -> bool {
        self.entry_pc_contains(entry, addr)
            .or_else(|| self.entry_ranges_contain(entry, addr, unit))
            .unwrap_or(false)
    }

    fn entry_pc_contains(&self, entry: &Entry, addr: &Addr) -> Option<bool> {
        let low = match entry.attr_value(DW_AT_low_pc).ok()?? {
            AttrValue::Addr(addr) => addr,
            _ => None?,
        };

        let high = match entry.attr_value(DW_AT_high_pc).ok()?? {
            AttrValue::Addr(addr) => addr,
            AttrValue::Udata(len) => low + len,
            _ => None?,
        };

        Some((low..high).contains(addr))
    }

    fn entry_ranges_contain(&self, entry: &Entry, addr: &Addr, unit: &Unit) -> Option<bool> {
        let AttrValue::RangeListsRef(offset) = entry.attr_value(DW_AT_ranges).ok()?? else {
            None?
        };

        self.ranges(unit, self.ranges_offset_from_raw(unit, offset))
            .and_then(|mut rs| rs.any(|r| Ok((r.begin..r.end).contains(addr))))
            .ok()
    }

    fn line_row_file(
        &self,
        row: &LineRow,
        header: &LineProgramHeader,
        unit: &Unit,
    ) -> Result<PathBuf, Error> {
        row.file(header)
            .ok_or_else(|| Error::AddrFileInfoMissing(Addr::from(row.address())))
            .and_then(|file| {
                Ok(match file.directory(header) {
                    Some(dir) if file.directory_index() != 0 => {
                        PathBuf::from(&*self.attr_lossy_string(unit, dir)?)
                    }
                    _ => PathBuf::default(),
                }
                .join(&*self.attr_lossy_string(unit, file.path_name())?))
            })
    }

    fn attr_lossy_string<'input>(
        &'input self,
        unit: &Unit<'input>,
        attr: AttrValue<'input>,
    ) -> Result<Cow<'_, str>, gimli::Error> {
        Ok(self.attr_string(unit, attr)?.to_string_lossy())
    }

    fn unit_from_offset(&self, addr: Addr, offset: DebugInfoOffset) -> Result<Unit, Error> {
        let unit_offset = UnitSectionOffset::from(offset);
        let mut headers = self.units().peekable();
        let header = loop {
            match (headers.next()?, headers.peek()?) {
                (Some(header), Some(next_header))
                    if (header.offset()..next_header.offset()).contains(&unit_offset) =>
                {
                    break header
                }
                (Some(header), None) if unit_offset > header.offset() => break header,
                (None, _) => Err(Error::AddrDebugInfoRefOffsetNofFound(addr))?,
                (_, _) => continue,
            };
        };
        Ok(self.unit(header)?)
    }

    fn unit_from_addr(&self, addr: &Addr) -> Result<Unit, Error> {
        let offset = self.debug_info_offset(addr)?;
        let header = self.debug_info.header_from_offset(offset)?;
        Ok(self.unit(header)?)
    }

    fn debug_info_offset(&self, addr: &Addr) -> Result<DebugInfoOffset, Error> {
        let contains = |addr| {
            move |arange: gimli::ArangeEntry| {
                arange
                    .address()
                    .checked_add(arange.length())
                    .map(|address_end| (arange.address()..address_end).contains(addr))
                    .ok_or(gimli::Error::InvalidAddressRange)
            }
        };

        self.debug_aranges
            .headers()
            .find_map(|header| {
                Ok(header
                    .entries()
                    .any(contains(addr))?
                    .then(|| header.debug_info_offset()))
            })?
            .ok_or(Error::AddrDebugInfoOffsetMissing(*addr))
    }
}
