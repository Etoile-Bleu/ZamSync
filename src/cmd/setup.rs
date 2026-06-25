use crate::cmd;
use std::fs;
use std::path::Path;

pub fn setup(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Validate arguments
    if args.len() < 4 {
        return Err("Usage: zamsync setup --hub <data-dir> [--bind <addr>]".into());
    }

    // Ensure correct flag
    let flag = &args[2];
    if flag != "--hub" {
        return Err("Only --hub mode is supported".into());
    }

    let data_dir = &args[3];
    let path = Path::new(data_dir);

    // Default bind address
    let mut bind_addr = "0.0.0.0:5000".to_string();

    // Robust flag parsing
    if let Some(pos) = args.iter().position(|x| x == "--bind") {
        if let Some(val) = args.get(pos + 1) {
            bind_addr = val.clone();
        } else {
            return Err("Missing value for --bind".into());
        }
    }

    println!("Setting up hub in directory: {}", data_dir);

    // Create directory if it doesn't exist
    if !path.exists() {
        fs::create_dir_all(path)?;
    }

    println!("[ok] directory ready");

    // Prevent overwrite
    let tls_dir = path.join("tls");
    if tls_dir.join("node.key").exists() {
        return Err("Error: node.key already exists. Aborting.".into());
    }

    // Call existing keygen logic
    let keygen_args = vec![args[0].clone(), "keygen".to_string(), data_dir.to_string()];

    cmd::keygen(&keygen_args)?;

    println!("[ok] keys generated");

    // Generate systemd unit file
    let service_content = format!(
        r#"[Unit]
Description=ZamSync Hub Node
After=network.target

[Service]
ExecStart=/usr/local/bin/zamsync serve {} {}
Restart=always
User=zamsync

[Install]
WantedBy=multi-user.target
"#,
        data_dir, bind_addr
    );

    let service_path = path.join("zamsync-hub.service");

    fs::write(&service_path, service_content)?;

    println!("[ok] systemd unit created -> {}", service_path.display());

    // Final checklist
    println!("\nNext steps:");
    println!("  1. Copy the unit:");
    println!(
        "     sudo cp {}/zamsync-hub.service /etc/systemd/system/",
        data_dir
    );
    println!("  2. Enable + start:");
    println!("     sudo systemctl enable --now zamsync-hub");
    println!("  3. For each clinic node:");
    println!("     copy {}/tls/ca.crt", data_dir);
    println!("     zamsync keygen ./clinic-data");
    println!("     zamsync sign ./clinic_data --ca {}", data_dir);
    println!("  Bind address: {}", bind_addr);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_setup_creates_service_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        let args = vec![
            "zamsync".to_string(),
            "setup".to_string(),
            "--hub".to_string(),
            path.clone(),
        ];

        let result = setup(&args);

        assert!(result.is_ok());
        assert!(dir.path().exists());

        //IMPORTANT: verify service file creation
        assert!(dir.path().join("zamsync-hub.service").exists());
    }

    #[test]
    fn test_prevent_overwrite() {
        let dir = tempdir().unwrap();
        let tls_dir = dir.path().join("tls");
        std::fs::create_dir_all(&tls_dir).unwrap();

        // simulate existing node.key
        std::fs::write(tls_dir.join("node.key"), "dummy").unwrap();

        let args = vec![
            "zamsync".to_string(),
            "setup".to_string(),
            "--hub".to_string(),
            dir.path().to_str().unwrap().to_string(),
        ];

        let result = setup(&args);

        assert!(result.is_err());
    }
}