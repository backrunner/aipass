//! Generate synthetic UI fixtures with the production compact-diff writer.
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut files: Vec<serde_json::Value> = serde_json::from_str(&input)?;
    for file in &mut files {
        let before = file["before"].as_str().ok_or("missing before")?;
        let content = file["content"].as_str().ok_or("missing content")?;
        let diff = aipass_config_writers::diff_preview_from(before, content);
        file["diff"] = diff.into();
    }
    println!("{}", serde_json::to_string_pretty(&files)?);
    Ok(())
}
