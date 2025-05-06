#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

extern crate winapi;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use slint::SharedString;
use slint::ComponentHandle;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use zip::ZipArchive;
use winapi::um::winuser::{MessageBeep, MB_OK};

slint::include_modules!();

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    pack_location: PathBuf,
    version: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
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

                if (Path::new("%AppData%/ModrinthApp/profiles")).exists() {
                    pack_location = rfd::FileDialog::new()
                        .set_directory(Path::new("%AppData%/ModrinthApp/profiles"))
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
                    };
                    println!("pack_location: {}", config.pack_location.display());

                    let file = File::create("config.json").unwrap();
                    serde_json::to_writer_pretty(file, &config).unwrap();
                    println!("wrote to file");

                    slint::invoke_from_event_loop(move ||  {
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
            // check for updates

            let file = File::open(Path::new("config.json")).unwrap();
            let reader = BufReader::new(file);
            let mut config: Config = serde_json::from_reader(reader).unwrap();
            let installed_version = config.version;
            let mut parts = installed_version.split(".");
            let installed_version = format!("v{}.{}", parts.next().unwrap(), parts.next().unwrap());

            let url = "https:/api.github.com/repos/JMBROGB666/The-OG3-Pack-1.20.1/releases/latest";
            let client = reqwest::blocking::Client::new();
            let release: Release = client
                .get(url)
                .header("User-Agent", "OG3-pack-updater")
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

                // download latest version and replace files
                let file: &Asset = release
                    .assets
                    .iter()
                    .find(|a| a.name.ends_with(".zip"))
                    .unwrap();
                println!("Downloading: {}", file.name);

                let main_clone3 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone3.upgrade();
                    main_clone.unwrap().set_spinnerload(0.2);
                })
                .unwrap();

                let mut resp = client
                    .get(&file.browser_download_url)
                    .header("User-Agent", "OG3-pack-updater")
                    .send()
                    .unwrap();
                // store file in buffer
                let mut buf = Vec::new();
                resp.read_to_end(&mut buf).unwrap();

                println!("Downloaded file size: {} bytes", buf.len());

                let main_clone4 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone4.upgrade();
                    main_clone.unwrap().set_spinnerload(0.3);
                })
                .unwrap();

                // drop the response if the borrow checker has not done it already to avoid using a large amount of memory
                drop(resp);

                let target_dir = config.pack_location.as_path().join("mods");

                // delete old mods folder
                if target_dir.exists() {
                    fs::remove_dir_all(&target_dir).unwrap();
                    println!("Removed existing mods")
                }

                let main_clone5 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone5.upgrade();
                    main_clone.unwrap().set_spinnerload(0.5);
                })
                .unwrap();

                // unpack the files and write them to the new folder
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

                // extract files from the inner .mrpack zip
                let reader = Cursor::new(mrpack_data);
                let mut inner_archive = ZipArchive::new(reader).unwrap();

                for i in 0..inner_archive.len() {
                    let mut file = inner_archive.by_index(i).unwrap();
                    let file_path = file.sanitized_name();

                    // Only process files that have overrides/mods in their path
                    let components: Vec<_> = file_path.components().collect();
                    let mods_index = components.windows(2).position(|w| {
                        w[0].as_os_str() == "overrides" && w[1].as_os_str() == "mods"
                    });

                    if let Some(index) = mods_index {
                        // Skip the overrides/mods prefix
                        let rel_path: PathBuf = components[index + 2..].iter().collect();
                        let outpath = target_dir.join(rel_path);

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

                // get config file

                let mut index_json: Option<Value> = None;

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

                // get all files from json

                let mut mod_files = Vec::new();

                if let Some(files) = json_file.get("files").and_then(|v| v.as_array()) {
                    for file_entry in files {
                        if let Some(path) = file_entry.get("path").and_then(|v| v.as_str()) {
                            if path.contains("mods/") {
                                // Get filename from path
                                let segments: Vec<&str> = path.split('/').collect();
                                if let Some(file_name) = segments.last() {
                                    // Get first download URL
                                    if let Some(downloads) =
                                        file_entry.get("downloads").and_then(|v| v.as_array())
                                    {
                                        if let Some(first_url) =
                                            downloads.get(0).and_then(|v| v.as_str())
                                        {
                                            mod_files.push((
                                                file_name.to_string(),
                                                first_url.to_string(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let main_clone9 = clone.clone();
                slint::invoke_from_event_loop(move || {
                    let main_clone = main_clone9.upgrade();
                    main_clone.unwrap().set_spinnerload(0.8);
                })
                .unwrap();

                println!("Collected {} mod URLs:", mod_files.len());

                for (file_name, url) in &mod_files {
                    let out_path = target_dir.join(file_name);
                    println!("Downloading {} to {:?}", url, out_path);

                    let mut resp = client
                        .get(url)
                        .header("User-Agent", "OG3-pack-updater")
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