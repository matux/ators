mod cli;

use atorsl::{data::Context, ext::object::File, read::Lookup};
use atorsl::{load_dwarf, load_object};
use cli::FromArgs;

fn main() -> anyhow::Result<()> {
    let context = Context::from_args(cli::build().get_matches());

    let (mmap, cow);
    let object = load_object!(context.object_path, mmap)?;

    load_dwarf!(object, cow)
        .lookup(object.vmaddr()?, context)?
        .into_iter()
        .for_each(|symbol| println!("{symbol}"));

    Ok(())
}
