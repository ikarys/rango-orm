use anyhow::{bail, Result};
use std::fs;
use std::path::Path;

pub fn run(backend: &str) -> Result<()> {
    let url = match backend {
        "postgres" => "postgres://user:password@localhost:5432/mydb",
        "sqlite" => "sqlite://db.sqlite3",
        other => bail!("Unknown backend '{}'. Use 'sqlite' or 'postgres'.", other),
    };

    if Path::new("rango.toml").exists() {
        bail!("rango.toml already exists — delete it first if you want to reinitialize.");
    }

    let content = format!(
        r#"[database]
url = "{url}"

[models]
src = "src"

[migrations]
dir = "migrations"
"#
    );

    fs::write("rango.toml", content)?;
    fs::create_dir_all("migrations")?;

    println!("✅ rango.toml created (backend: {})", backend);
    println!("✅ migrations/ directory created");
    println!();
    println!("Next steps:");
    println!("  1. Define your models with #[derive(Model)]");
    println!("  2. Run `rango makemigrations` to generate SQL migrations");
    println!("  3. Run `rango migrate` to apply them");

    Ok(())
}
