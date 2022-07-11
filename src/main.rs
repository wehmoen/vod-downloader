#[macro_use]
extern crate fstrings;

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Gronkh.TV VOD Downloader
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// VOD ID
    #[clap(long, value_parser)]
    vod_id: String,
    /// Path to ffmpeg
    #[clap(long, value_parser, default_value="")]
    ffmpeg_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde()]
struct PlaylistInfo {
    playlist_url: String,
}

struct PlaylistVariant {
    bandwith: String,
    framerate: String,
    name: String,
    resolution: String,
}

fn get_playlist_variant(variant: &str) -> PlaylistVariant {
    let parts: Vec<&str> = variant.split("p").collect();
    let quality = parts[0].to_owned();
    let framerate: String = if parts.len() == 2 { parts[1].to_owned() } else { "30".to_string() };
    let bandwith: String = if parts[0] == "1080" { "6000000".to_string() } else if parts[0] == "720" { "2600000".to_string() } else { "1000000".to_string() };
    let resolution: String = if parts[0] == "1080" { "1920x1080".to_string() } else if parts[0] == "720" { "1080x720".to_string() } else { "640x360".to_string() };
    let name = if parts.len() == 2 { variant.to_owned() } else { f!("{quality}p") };

    PlaylistVariant {
        bandwith,
        framerate,
        name,
        resolution,
    }
}

fn hls_to_mp4(args: &Args, variant: &str) {
    let path = args.ffmpeg_path.as_ref().unwrap();
    if std::path::Path::new(&path).exists() == true {
        let mut te = Command::new(&path);
        let input = f!("./gronkhtv/{args.vod_id}/{variant}/index.m3u8");
        let output = f!("./gronkhtv/{args.vod_id}/{variant}.mp4");
        te.args(&[
            "-y",
            "-i",
            &input,
            "-c",
            "copy",
            &output
        ]);
        te.spawn().expect("FFmpeg exited with a non 0 status code!");
    } else {
        println!("{}", "FFMPEG not found or not specified. Skipping HLS => MP4 conversion");
    }
}

fn create_master_playlist(variants: &Vec<&str>) -> String {
    let mut playlist: Vec<String> = vec![];
    playlist.push("#EXTM3U".to_string());
    playlist.push("#EXT-X-VERSION:3".to_string());

    for v in variants {
        let details: PlaylistVariant = get_playlist_variant(v);
        playlist.push(f!("#EXT-X-STREAM-INF:BANDWIDTH={details.bandwith},RESOLUTION={details.resolution},FRAMERATE={details.framerate},CODECS=\"avc1.4D402A,mp4a.40.2\",NAME=\"{details.name}\""));
        playlist.push(f!("{v}/index.m3u8"));
    }

    playlist.join("\r\n")
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();

    let gtv_playlist_url = f!("https://api.gronkh.tv/v1/video/playlist?episode={args.vod_id}");

    let client = reqwest::Client::new();

    let playlist_request: Result<PlaylistInfo, reqwest::Error> = client.get(&gtv_playlist_url)
        .send()
        .await
        .unwrap()
        .json().await;

    let playlist_url = match playlist_request {
        Ok(res) => res.playlist_url,
        Err(e) => panic!("{} {}", e, &gtv_playlist_url)
    };

    let playlist_variants_request = client.get(&playlist_url)
        .send()
        .await
        .unwrap()
        .text().await.unwrap();

    let variants: Vec<&str> = playlist_variants_request.split("\r\n").collect();

    let mut variant_urls: Vec<String> = vec![];
    let mut variant_names: Vec<&str> = vec![];

    for url in variants {
        if url.starts_with("https") {
            variant_urls.push(String::from(url));
        }
    }

    for variant in &variant_urls {
        let url_parts: Vec<&str> = variant.split("/").collect();
        let quality = url_parts[5];
        variant_names.push(&quality);
        let id = { url_parts[4] };
        let ts_base: String = f!("https://01.cdn.vod.farm/transcode/{id}/{quality}/");
        let output = f!("gronkhtv/{args.vod_id}/{quality}");

        fs::create_dir_all(output).expect("Failed to create output directory!");

        let playlist: String = client.get(variant)
            .send()
            .await
            .unwrap()
            .text().await.unwrap();

        let mut ts_files: Vec<String> = vec![];
        let lines: Vec<&str> = playlist.split("\n").collect();

        for line in lines {
            if line.ends_with(".ts") {
                ts_files.push(String::from(line));
            }
        }

        let playlist_out = f!("gronkhtv/{args.vod_id}/{quality}/index.m3u8");

        fs::write(playlist_out, &playlist).expect("Failed to write playlist!");

        for file in ts_files {
            let mut full_url = ts_base.clone();
            full_url.push_str(&file);

            let output_file = f!("gronkhtv/{args.vod_id}/{quality}/{file}");

            if std::path::Path::new(&output_file).exists() == false {
                let mut out_file = std::fs::File::create(&output_file).unwrap();

                let ts_file = client.get(full_url).send().await.unwrap().bytes().await.unwrap();
                out_file.write_all(&ts_file).expect("Failed to write ts file.");
                println!("Downloaded ./gronkhtv/{}/{}/{}", args.vod_id, quality, file);
            }
        }
        hls_to_mp4(&args, &quality);
    }
    let master_playlist = create_master_playlist(&variant_names);
    let master_playlist_output = f!("gronkhtv/{args.vod_id}/index.m3u8");
    fs::write(master_playlist_output, master_playlist).expect("Failed to write master playlist");
}
