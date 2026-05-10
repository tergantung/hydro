use std::fs;
use std::io;
use std::path::Path;
use zip::ZipArchive;

const API_URL: &str = "https://api.github.com/repos/tergantung/hydro-rs/releases/latest";
const CURRENT_VERSION: &str = "v0.1.0-beta";

#[derive(serde::Deserialize, Debug)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Deserialize, Debug)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

fn main() {
    println!("=== Hydro Auto-Updater ===");
    println!("Checking for updates from GitHub...");

    let mut resp = match ureq::get(API_URL)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .header("Accept", "application/vnd.github.v3+json")
        .call() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to check updates: {}", e);
            eprintln!("(Pastikan repository tergantung/hydro-rs sudah di-set ke Public dan ada Release terbaru)");
            wait_for_input();
            return;
        }
    };

    let release: GithubRelease = match resp.body_mut().read_json() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse GitHub release data: {}", e);
            wait_for_input();
            return;
        }
    };

    let hydro_exists = if cfg!(target_os = "windows") {
        Path::new("Hydro.exe").exists()
    } else {
        Path::new("./Hydro").exists()
    };

    if release.tag_name == CURRENT_VERSION && hydro_exists {
        println!("Hydro is up to date ({})", CURRENT_VERSION);
        launch_hydro();
        return;
    }

    if !hydro_exists {
        println!("Hydro not found. Starting initial installation...");
    } else {
        println!("New version found: {}", release.tag_name);
    }
    
    // Find the zip asset
    let asset = match release.assets.iter().find(|a| a.name.ends_with(".zip")) {
        Some(a) => a,
        None => {
            eprintln!("No .zip update file found in the latest GitHub release!");
            wait_for_input();
            return;
        }
    };

    println!("Downloading update from: {}", asset.browser_download_url);

    let mut download_resp = match ureq::get(&asset.browser_download_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .header("Accept", "application/octet-stream")
        .call() {
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
