use binrw::BinResult;
use std::io;
use nativestart::recompress::recompress;

fn main() -> BinResult<()> {
    let mut hasher = blake3::Hasher::new();
    recompress(&mut io::stdin(), &mut hasher)?;

    println!("{}", hasher.count());
    println!("{}", String::from(hasher.finalize().to_hex().as_str()));
    Ok(())
}
