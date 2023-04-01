use crate::{
    ext::gimli::{ArangeEntry, DebuggingInformationEntry},
    format, Addr, Context, Error,
};
use fallible_iterator::FallibleIterator;
use gimli::{DW_TAG_subprogram, DebugInfoOffset, Dwarf, EndianSlice, RunTimeEndian};

type Unit<'input> = gimli::Unit<EndianSlice<'input, RunTimeEndian>, usize>;
type UnitHeader<'input> = gimli::UnitHeader<EndianSlice<'input, RunTimeEndian>, usize>;
type Entry<'abbrev, 'unit, 'input> =
    gimli::DebuggingInformationEntry<'abbrev, 'unit, EndianSlice<'input, RunTimeEndian>, usize>;

pub trait Lookup {
    fn lookup(&self, vmaddr: Addr, context: &Context) -> Result<Vec<String>, Error>;
    fn lookup_addr(&self, address: Addr, context: &Context) -> Result<String, Error>;

    fn symbolicate(&self, entry: &Entry, unit: &Unit) -> Result<String, Error>;
    fn unit_from_addr(&self, addr: Addr) -> Result<(UnitHeader, Unit), Error>;
    fn debug_info_offset_from_addr(&self, addr: Addr) -> Result<DebugInfoOffset, Error>;
}

impl<'data> Lookup for Dwarf<EndianSlice<'_, RunTimeEndian>> {
    fn lookup(&self, vmaddr: Addr, context: &Context) -> Result<Vec<String>, Error> {
        fallible_iterator::convert(
            context
                .addrs
                .to_owned()
                .into_iter()
                .map(|addr| self.lookup_addr(addr - context.loadaddr + vmaddr, &context)),
        )
        .collect()
    }

    fn lookup_addr(&self, addr: Addr, context: &Context) -> Result<String, Error> {
        let (header, unit) = self.unit_from_addr(addr)?;
        let mut entries = unit.entries();

        loop {
            let Some((_, entry)) = entries.next_dfs()? else {
                break Err(Error::AddressNotFound(addr))
            };

            if context.verbose {
                println!("{}", format::entry(entry, self, &header, &unit));
            }

            match entry.pc() {
                Some(pc) if entry.tag() == DW_TAG_subprogram && pc.contains(&addr) => {
                    break self.symbolicate(entry, &unit)
                }
                _ => continue,
            }
        }
    }

    fn symbolicate(&self, entry: &Entry, unit: &Unit) -> Result<String, Error> {
        entry
            .linkage_name()
            .or_else(|| entry.name())
            .ok_or(Error::AddressHasNoSymbol)
            .and_then(|value| {
                Ok(self
                    .attr_string(&unit, value)
                    .map_err(Error::Gimli)?
                    .to_string_lossy()
                    .to_string())
            })
    }

    fn unit_from_addr(&self, addr: Addr) -> Result<(UnitHeader, Unit), Error> {
        let offset = self.debug_info_offset_from_addr(addr)?;
        let header = self.debug_info.header_from_offset(offset)?;
        Ok((header, self.unit(header)?))
    }

    fn debug_info_offset_from_addr(&self, addr: Addr) -> Result<DebugInfoOffset, Error> {
        self.debug_aranges
            .headers()
            .find_map(|header| {
                Ok(if header.entries().any(|entry| entry.contains(addr))? {
                    Some(header.debug_info_offset())
                } else {
                    None
                })
            })?
            .ok_or(Error::NoDebugOffsetInAddress(addr))
    }
}
