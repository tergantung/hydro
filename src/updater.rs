use std::fs;
use std::io;
use std::path::Path;
use zip::ZipArchive;

const CONFIG_URL: &str = "https://gist.githubusercontent.com/user/gist_id/raw/config.json";
const CURRENT_VERSION: &str = "0.1.0-beta";

#[derive(serde::Deserialize, Debug)]
struct RemoteConfig {
    latest_version: String,
    download_url: String,
}

fn main() {
    println!("=== Hydro Auto-Updater ===");
    println!("Checking for updates...");

    let mut resp = match ureq::get(CONFIG_URL).call() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to check updates: {}", e);
            wait_for_input();
            return;
        }
    };

    let config: RemoteConfig = match resp.body_mut().read_json() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse update config: {}", e);
            wait_for_input();
            return;
        }
    };

    let hydro_exists = if cfg!(target_os = "windows") {
        Path::new("Hydro.exe").exists()
    } else {
        Path::new("./Hydro").exists()
    };

    if config.latest_version == CURRENT_VERSION && hydro_exists {
        println!("Hydro is up to date (v{})", CURRENT_VERSION);
        launch_hydro();
        return;
    }

    if !hydro_exists {
        println!("Hydro not found. Starting initial installation...");
    } else {
        println!("New version found: v{}", config.latest_version);
    }
    
    println!("Downloading update from: {}", config.download_url);

    let mut download_resp = match ureq::get(&config.download_url).call() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to download update: {}", e);
            wait_for_input();
            return;
        }
    };

    let mut temp_file = tempfile::tempfile().expect("Failed to create temp file");
    // In ureq 3.x, Body implements Read if we use as_reader() but we can also copy directly
    io::copy(&mut download_resp.body_mut().as_reader(), &mut temp_file).expect("Failed to save download");

    println!("Extracting update...");
    let mut archive = ZipArchive::new(temp_file).expect("Failed to open zip archive");

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let outpath = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };

        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath).unwrap();
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(&p).unwrap();
                }
            }
            let mut outfile = fs::File::create(&outpath).unwrap();
            io::copy(&mut file, &mut outfile).unwrap();
        }
    }

    println!("Update installed successfully!");
    launch_hydro();
}

fn launch_hydro() {
    println!("Launching Hydro...");
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("Hydro.exe").spawn();
    #[cfg(not(target_os = "windows"))]
    let _ = std::process::Command::new("./Hydro").spawn();
    
    std::process::exit(0);
}

fn wait_for_input() {
    println!("\nPress Enter to exit...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
}
