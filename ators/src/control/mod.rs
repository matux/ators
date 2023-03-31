use gimli::{EndianSlice, RunTimeEndian};

pub mod dump;
pub mod format;
pub mod lookup;

pub use dump::Dump;
pub use lookup::Lookup;

pub type Dwarf<'a> = gimli::Dwarf<EndianSlice<'a, RunTimeEndian>>;
pub type Header<'a> = gimli::UnitHeader<EndianSlice<'a, RunTimeEndian>, usize>;
pub type Entry<'a> =
    gimli::DebuggingInformationEntry<'a, 'a, EndianSlice<'a, RunTimeEndian>, usize>;
pub type Attr<'a> = gimli::Attribute<EndianSlice<'a, RunTimeEndian>>;
pub type Unit<'a> = gimli::Unit<EndianSlice<'a, RunTimeEndian>, usize>;

#[macro_export]
macro_rules! load_dwarf {
    ($object:expr, $binding:ident) => {{
        $binding = gimli::Dwarf::load(|section_id| -> anyhow::Result<std::borrow::Cow<[u8]>> {
            Ok($object
                .section_by_name(section_id.name())
                .and_then(|section| section.uncompressed_data().ok())
                .unwrap_or(std::borrow::Cow::Borrowed(&[][..])))
        })?;

        $binding.borrow(|section| gimli::EndianSlice::new(&*section, $object.runtime_endian()))
    }};
}
