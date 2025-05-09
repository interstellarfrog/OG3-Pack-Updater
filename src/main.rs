#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

extern crate winapi;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use sha2::Sha512;
use slint::ComponentHandle;
use slint::SharedString;
use std::env;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use winapi::um::winuser::{MessageBeep, MB_OK};
use zip::ZipArchive;

slint::include_modules!();

/// The main config [Struct] for the updater
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Config {
    /// The location of the installed pack
    pack_location: PathBuf,
    /// The version of the installed pack
    version: String,
    /// If all files should be installed regardless (no hash rate limiting)
    redownload_all: bool,
}

/// [Struct] for holding a github release
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

/// [Struct] for holding a github file
#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// [Struct] for holding information about files
#[derive(Clone)]
struct FileInfo {
    /// The name of the file
    name: String,
    /// The download URL of the file
    url: Option<String>,
    /// The SHA512 hash of the file
    hash: Option<String>,
}

impl FileInfo {
    pub fn new(name: String, url: Option<String>, hash: Option<String>) -> Self {
        Self { name, url, hash }
    }
}

/// Computes the SHA512 hash of a file at a [Path]
fn compute_sha512_for_file<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    let mut file = BufReader::new(File::open(path)?);
    let mut hasher = Sha512::new();
    let mut buffer = [0u8; 8192];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    Ok(format!("{:x}", hasher.finalize()).to_lowercase())
}

/// Computes hashes using [compute_sha512_for_file] for all files in a [PathBuf] that is a directory
fn get_all_files_with_hashes(root_dir: PathBuf) -> Result<Vec<FileInfo>, std::io::Error> {
    let mut handles = Vec::new();
    let (tx, rx) = mpsc::channel();

    for entry_result in fs::read_dir(root_dir)? {
        let entry = entry_result?;
        let path = entry.path();

        if path.is_file() {
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                if let Ok(hash) = compute_sha512_for_file(&path) {
                    let _ = tx.send(Some(FileInfo::new(
                        path.file_name().unwrap().to_str().unwrap().to_string(),
                        None,
                        Some(hash),
                    )));
                } else {
                    let _ = tx.send(None);
                }
            }));
        }
    }

    drop(tx);

    let result: Vec<FileInfo> = rx.into_iter().filter_map(|opt| opt).collect();

    for handle in handles {
        let _ = handle.join();
    }

    Ok(result)
}

/// This function takes the json file from the mrpack and returns all the mods that may need to be downloaded with there information
///
/// This version includes hashes in the [FileInfo]
fn process_files_threaded_hash(json_file: &serde_json::Value) -> Vec<FileInfo> {
    // Convert files to vector for parallel processing
    let files: Vec<_> = json_file
        .get("files")
        .and_then(|v| v.as_array())
        .map(|arr| arr.into_iter().collect::<Vec<_>>())
        .unwrap_or_default();

    files
        .into_par_iter()
        .filter_map(|file_entry| {
            // Filter entries without valid paths containing "mods/"
            if let Some(path) = file_entry.get("path").and_then(|v| v.as_str()) {
                if !path.contains("mods/") {
                    return None; // Exit early if path doesn't contain "mods/"
                }

                // Extract filename
                let segments: Vec<&str> = path.split('/').collect();
                if let Some(file_name) = segments.last() {
                    // Process downloads array
                    if let Some(downloads) = file_entry.get("downloads").and_then(|v| v.as_array())
                    {
                        if let Some(url) = downloads.get(0).and_then(|v| v.as_str()) {
                            if let Some(hashes) =
                                file_entry.get("hashes").and_then(|v| v.as_object())
                            {
                                if let Some(hash) = hashes.get("sha512").and_then(|v| v.as_str()) {
                                    println!("Acquired SHA512: {:#?}", hash);
                                    return Some(FileInfo {
                                        name: file_name.to_string(),
                                        url: Some(url.to_string()),
                                        hash: Some(hash.to_string()),
                                    });
                                }
                            }
                        } else {
                            return None;
                        }
                    }
                }
            }
            None
        })
        .collect()
}

/// This function takes the json file from the mrpack and returns all the mods that may need to be downloaded with there information
///
/// This Version does not include hashes in the [FileInfo]
fn process_files_threaded(json_file: &serde_json::Value) -> Vec<FileInfo> {
    // Convert files to vector for parallel processing
    let files: Vec<_> = json_file
        .get("files")
        .and_then(|v| v.as_array())
        .map(|arr| arr.into_iter().collect::<Vec<_>>())
        .unwrap_or_default();

    files
        .into_par_iter()
        .filter_map(|file_entry| {
            // Filter entries without valid paths containing "mods/"
            if let Some(path) = file_entry.get("path").and_then(|v| v.as_str()) {
                if !path.contains("mods/") {
                    return None; // Exit early if path doesn't contain "mods/"
                }
                // Extract filename
                let segments: Vec<&str> = path.split('/').collect();
                if let Some(file_name) = segments.last() {
                    // Process downloads array
                    if let Some(downloads) = file_entry.get("downloads").and_then(|v| v.as_array())
                    {
                        if let Some(url) = downloads.get(0).and_then(|v| v.as_str()) {
                            return Some(FileInfo {
                                name: file_name.to_string(),
                                url: Some(url.to_string()),
                                hash: None,
                            });
                        }
                        return None;
                    }
                }
            }
            None
        })
        .collect()
}

fn main() {
    if File::open("config.json").is_err() {
        // If there is no config file
        let setup = SetupWindow::new().unwrap();

        let setup_weak = setup.as_weak();

        setup.on_setup(move || {
            // When get started is clicked
            let setup_clone = setup_weak.clone();
            thread::spawn(move || {
                // get pack location from user
                let pack_location;

                if (PathBuf::from(std::env::var("APPDATA").unwrap())
                    .join("ModrinthApp")
                    .join("profiles"))
                .exists()
                {
                    pack_location = rfd::FileDialog::new()
                        .set_directory(
                            PathBuf::from(std::env::var("APPDATA").unwrap())
                                .join("ModrinthApp")
                                .join("profiles"),
                        )
                        .set_title("Select the pack folder located inside of profiles folder")
                        .pick_folder();
                } else {
                    pack_location = rfd::FileDialog::new()
                        .set_title("Select the pack folder located inside of profiles folder")
                        .pick_folder();
                }

                if !pack_location.is_none() {
                    // if they selected a folder, get the path
                    let pack_location: PathBuf = pack_location.unwrap();

                    let temp_string = pack_location.display().to_string();

                    let temp_string = temp_string.split(" ");

                    // this should be the version unless if someone did something stupid
                    let version = temp_string.last().unwrap().to_string();

                    let config = Config {
                        pack_location,
                        version,
                        redownload_all: false,
                    };
                    println!("pack_location: {}", config.pack_location.display());

                    let file = File::create("config.json").unwrap();
                    serde_json::to_writer_pretty(file, &config).unwrap();
                    println!("wrote to file");

                    slint::invoke_from_event_loop(move || {
                        println!("inside event loop");

                        println!("got lock");

                        // close window
                        setup_clone
                            .unwrap()
                            .window()
                            .dispatch_event(slint::platform::WindowEvent::CloseRequested);
                    })
                    .unwrap();
                } // else do nothing
            });
        });

        setup.on_sitelink(move || {
            let _ = open::that("https://og3.infy.uk/");
        });

        setup.run().unwrap();
    }

    let mainwindow = MainWindow::new().unwrap();
    let main_weak = mainwindow.as_weak();

    mainwindow.on_start(move || {
        let clone = main_weak.clone();
        thread::spawn(move || {
            // kill modrinth app to stop it from messing with the mods
            println!(
                "{:#?}",
                std::process::Command::new("taskkill")
                    .args(["/F", "/IM", "Modrinth App.exe"])
                    .output()
            );

            // check for updates
            let file = File::open(Path::new("config.json")).unwrap();
            let reader = BufReader::new(file);
            let mut config: Config = serde_json::from_reader(reader).unwrap();
            let installed_version = config.version;
            let mut parts = installed_version.split(".");
            let installed_version = format!("v{}.{}", parts.next().unwrap(), parts.next().unwrap());
            let client = reqwest::blocking::Client::new();
            let release: Release = client
                .get("https:/api.github.com/repos/JMBROGB666/The-OG3-Pack-1.20.1/releases/latest")
                .header("User-Agent", "interstellarfrog/OG3-pack-updater")
                .send()
                .unwrap()
                .json()
                .unwrap();

            println!("latest version = {}", release.tag_name);
            println!("installed version = {}", installed_version);

            if installed_version == release.tag_name {
                // notify user that update is not needed
                let main_clone1 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone1.upgrade();
                    main_clone
                        .unwrap()
                        .set_update_available(SharedString::from("false"));
                })
                .unwrap();
            } else {
                println!("downloading pack");

                let main_clone2 = clone.clone();
                let main_clones = clone.clone();
                slint::invoke_from_event_loop(move || {
                    main_clone2
                        .unwrap()
                        .set_update_available(SharedString::from("true"));

                    main_clones.unwrap().set_spinnerload(0.1);
                })
                .unwrap();

                // download latest version of pack zip
                let file: &Asset = release
                    .assets
                    .iter()
                    .find(|a| a.name.ends_with(".zip"))
                    .unwrap();

                let main_clone3 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone3.upgrade();
                    main_clone.unwrap().set_spinnerload(0.2);
                })
                .unwrap();

                //check if we have the file
                let file_path = Path::new("./cache").join(&file.name);

                let mut buf = Vec::new();

                if file_path.exists() {
                    println!("Using cached version of {}", file.name);
                    buf = fs::read(&file_path).unwrap();
                } else {
                    println!("Downloading: {}", file.name);

                    let mut resp = client
                        .get(&file.browser_download_url)
                        .header("User-Agent", "interstellarfrog/OG3-pack-updater")
                        .send()
                        .unwrap();

                    // store file in buffer
                    resp.read_to_end(&mut buf).unwrap();

                    // cache the file
                    fs::create_dir_all("./cache").unwrap();
                    fs::write(&file_path, &buf).unwrap();

                    // drop the response if the borrow checker has not done it already to avoid using a large amount of memory
                    drop(resp);
                }

                println!("File size: {} bytes", buf.len());

                let main_clone4 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone4.upgrade();
                    main_clone.unwrap().set_spinnerload(0.3);
                })
                .unwrap();

                let target_dir = config.pack_location.as_path().join("mods");

                // get [FileInfo] for all installed mods
                let local_modinfo = get_all_files_with_hashes(target_dir.clone()).unwrap();

                let main_clone5 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone5.upgrade();
                    main_clone.unwrap().set_spinnerload(0.5);
                })
                .unwrap();

                let reader = Cursor::new(buf);
                let mut archive = ZipArchive::new(reader).unwrap();

                let mut mrpack_data = Vec::new();

                // find and read the .mrpack file
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i).unwrap();
                    if file.name().ends_with(".mrpack") {
                        file.read_to_end(&mut mrpack_data).unwrap();
                        println!(
                            "Found .mrpack file: {} size: {} bytes",
                            file.name(),
                            mrpack_data.len()
                        );
                        break;
                    }
                }

                if mrpack_data.is_empty() {
                    panic!("No .mrpack file found in the archive");
                }

                let reader = Cursor::new(mrpack_data);
                let mut inner_archive = ZipArchive::new(reader).unwrap();
                let mut index_json: Option<Value> = None;

                // Get the index.json file
                for i in 0..inner_archive.len() {
                    let mut file = inner_archive.by_index(i).unwrap();
                    if file.name() == "modrinth.index.json" {
                        let mut contents = String::new();
                        file.read_to_string(&mut contents).unwrap();
                        index_json = Some(serde_json::from_str(&contents).unwrap());
                        break;
                    }
                }

                let json_file = index_json.unwrap();

                println!("Getting mods from json file");

                let mod_files;

                if config.redownload_all {
                    // without hash
                    mod_files = process_files_threaded(&json_file);
                } else {
                    // with hash
                    mod_files = process_files_threaded_hash(&json_file);
                }

                println!("Checking what mods to delete and download");

                let mut to_download = Vec::new();

                // compile a list of what mods to download
                if config.redownload_all {
                    let _ = fs::remove_dir_all(&target_dir);
                    to_download = mod_files;
                } else {
                    // build a set of names for mod_files
                    let faster_names: std::collections::HashSet<_> =
                        mod_files.iter().map(|item| &item.name).collect();
                    let faster_hashes: std::collections::HashSet<_> =
                        mod_files.iter().map(|item| &item.hash).collect();
                    let faster_installed_hashes: std::collections::HashSet<_> =
                        local_modinfo.iter().map(|item| &item.hash).collect();

                    for file_info in local_modinfo.clone() {
                        // if the mod not expected to be installed
                        if !faster_names.contains(&file_info.name)
                            || !faster_hashes.contains(&file_info.hash)
                        {
                            // this file is no longer in the modpack or the user installed the file manually, so delete it
                            let _ = fs::remove_file(target_dir.join(file_info.name));
                        }
                    }

                    for file_info in mod_files {
                        // if the file is not installed or corrupt
                        if !faster_installed_hashes.contains(&file_info.hash) {
                            // add to downloads
                            to_download.push(file_info);
                        }
                    }
                }

                // install all of the .mrpack mod files as we alreay have them from the zip
                for i in 0..inner_archive.len() {
                    let mut file = inner_archive.by_index(i).unwrap();
                    let file_path = file.mangled_name();
                    let components: Vec<_> = file_path.components().collect();

                    // Find which override subdirectory the file belongs to
                    let (subdir, index) = if let Some(i) = components
                        .windows(2)
                        .position(|w| w[0].as_os_str() == "overrides" && w[1].as_os_str() == "mods")
                    {
                        ("mods", i)
                    } else if let Some(i) = components.windows(2).position(|w| {
                        w[0].as_os_str() == "overrides" && w[1].as_os_str() == "shaderpacks"
                    }) {
                        ("shaderpacks", i)
                    } else if let Some(i) = components.windows(2).position(|w| {
                        w[0].as_os_str() == "overrides" && w[1].as_os_str() == "resourcepacks"
                    }) {
                        ("resourcepacks", i)
                    } else {
                        ("", 0)
                    };

                    if !subdir.is_empty() {
                        // Build relative path and final extraction path
                        let rel_path: PathBuf = components[index + 2..].iter().collect();
                        let outpath = config.pack_location.as_path().join(subdir).join(rel_path);

                        if file.is_dir() {
                            fs::create_dir_all(&outpath).unwrap();
                        } else {
                            if let Some(parent) = outpath.parent() {
                                fs::create_dir_all(parent).unwrap();
                            }
                            let mut outfile = File::create(&outpath).unwrap();
                            std::io::copy(&mut file, &mut outfile).unwrap();
                            println!("Extracted: {:?}", outpath);
                        }
                    }
                }

                let main_clone8 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone8.upgrade();
                    main_clone.unwrap().set_spinnerload(0.7);
                })
                .unwrap();

                println!("Extracted mods folder to {:?}", target_dir);

                let main_clone9 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone9.upgrade();
                    main_clone.unwrap().set_spinnerload(0.8);
                })
                .unwrap();

                println!("Collected {} mod URLs to download:", to_download.len());

                // download and install mods
                for fileinfo in &to_download {
                    let out_path = target_dir.join(fileinfo.name.clone());
                    println!(
                        "Downloading {} to {:?}",
                        fileinfo.url.clone().unwrap(),
                        out_path
                    );
                    let mut resp = client
                        .get(fileinfo.url.clone().unwrap())
                        .header("User-Agent", "interstellarfrog/OG3-pack-updater")
                        .send()
                        .unwrap();

                    let mut out_file = fs::File::create(&out_path).unwrap();
                    std::io::copy(&mut resp, &mut out_file).unwrap();
                }

                let main_clone9 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone9.upgrade();
                    main_clone.unwrap().set_spinnerload(0.9);
                })
                .unwrap();

                // get new pack version and update config
                // we do this last as if the user cancels the downloads it should not break everything

                let new_version = json_file["versionId"].as_str().unwrap();

                config.version = new_version.to_string();

                fs::write(
                    "config.json",
                    serde_json::to_string_pretty(&config).unwrap(),
                )
                .unwrap();

                let main_clone6 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone6.upgrade();
                    main_clone.unwrap().set_spinnerload(1.0);
                })
                .unwrap();

                let main_clone7 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone7.upgrade();
                    main_clone
                        .unwrap()
                        .set_update_available(SharedString::from("done"))
                })
                .unwrap();

                #[cfg(target_os = "windows")]
                unsafe {
                    MessageBeep(MB_OK);
                }
            }
        });
    });
    mainwindow.run().unwrap();
}
