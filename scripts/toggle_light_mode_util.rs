use anyhow::Result;
use redb::{Database, ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json;
use std::env;
use std::path::PathBuf;

const STATE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("queue");

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueState {
    #[serde(default)]
    light_mode: bool,
    #[serde(flatten)]
    other: serde_json::Value,
}

fn get_db_path() -> PathBuf {
    dirs::config_dir()
        .unwrap()
        .join("nokkvi")
        .join("app.redb")
}

fn set_light_mode(enable: bool) -> Result<()> {
    let db_path = get_db_path();
    
    if !db_path.exists() {
        eprintln!("Database not found: {}", db_path.display());
        return Err(anyhow::anyhow!("Database not found"));
    }
    
    let db = Database::open(&db_path)?;
    
    // Read current state
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(STATE_TABLE)?;
    
    let current_state = match table.get("user_settings")? {
        Some(value) => {
            let bytes = value.value();
            let mut state: serde_json::Value = serde_json::from_slice(bytes)?;
            
            // Update light_mode field
            if let Some(obj) = state.as_object_mut() {
                obj.insert("light_mode".to_string(), serde_json::Value::Bool(enable));
            }
            
            state
        }
        None => {
            eprintln!("No state found in database");
            return Err(anyhow::anyhow!("No state found"));
        }
    };
    
    drop(table);
    drop(read_txn);
    
    // Write updated state
    let serialized = serde_json::to_vec(&current_state)?;
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(STATE_TABLE)?;
        table.insert("user_settings", serialized.as_slice())?;
    }
    write_txn.commit()?;
    
    let mode_str = if enable { "light" } else { "dark" };
    println!("✓ Set mode to: {}", mode_str);
    
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() != 2 {
        eprintln!("Usage: {} <true|false>", args[0]);
        eprintln!("  true  - Enable light mode");
        eprintln!("  false - Enable dark mode");
        std::process::exit(1);
    }
    
    let enable = match args[1].to_lowercase().as_str() {
        "true" | "1" | "yes" | "light" => true,
        "false" | "0" | "no" | "dark" => false,
        _ => {
            eprintln!("Invalid argument. Use 'true' or 'false'");
            std::process::exit(1);
        }
    };
    
    set_light_mode(enable)?;
    
    Ok(())
}
