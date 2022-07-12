#[macro_use]
extern crate fstrings;

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{fmt, fs};
use std::fmt::Formatter;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use reqwest::Client;
use indicatif::{ProgressBar, ProgressStyle};

/// Gronkh.TV VOD Downloader
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// VOD ID
    #[clap(long, value_parser)]
    vod_id: String,
    /// Path to ffmpeg
    #[clap(long, value_parser, default_value = "")]
    ffmpeg_path: String,
    /// Output Path
    #[clap(long, value_parser, default_value = "gronkhtv")]
    output_path: String,

}

#[derive(Serialize, Deserialize, Debug)]
#[serde()]
struct PlaylistInfo {
    playlist_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde()]
struct VideoInfo {
    title: String,
    preview_url: String,
    created_at: String,
    episode: i32,
}

impl fmt::Display for VideoInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let episode: String = self.episode.to_string();
        write!(f, "VOD: [{}] - \"{}\"", episode, self.title)
    }
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

async fn get_video_info(client: &Client, args: &Args) -> VideoInfo {
    let info_url = f!("https://api.gronkh.tv/v1/video/info?episode={args.vod_id}");
    let info: Result<VideoInfo, reqwest::Error> = client.get(&info_url)
        .send()
        .await
        .unwrap()
        .json().await;

    match info {
        Ok(res) => res,
        Err(e) => panic!("{} {}", e, &info_url)
    }
}

async fn get_playlist_info(client: &Client, args: &Args) -> PlaylistInfo {
    let info_url = f!("https://api.gronkh.tv/v1/video/playlist?episode={args.vod_id}");
    let info: Result<PlaylistInfo, reqwest::Error> = client.get(&info_url)
        .send()
        .await
        .unwrap()
        .json().await;

    match info {
        Ok(res) => res,
        Err(e) => panic!("{} {}", e, &info_url)
    }
}

async fn web_get_text(client: &Client, url: &str) -> String {
    client.get(url)
        .send()
        .await
        .unwrap()
        .text().await.unwrap()
}

fn hls_to_mp4(args: &Args, variant: &str, ts_file_len: u64) {
    if Path::new(&args.ffmpeg_path).exists() == true {
        let input = f!("./{args.output_path}/{args.vod_id}/{variant}/index.m3u8");
        let output = f!("./{args.output_path}/{args.vod_id}/{variant}.mp4");
        let ffmpeg = Command::new(&args.ffmpeg_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(
                &[
                    "-y",
                    "-hide_banner",
                    "-i",
                    &input,
                    "-c",
                    "copy",
                    &output
                ]
            )
            .spawn()
            .unwrap()
            ;

        let reader = BufReader::new(ffmpeg.stderr.unwrap());

        let bar = ProgressBar::new(ts_file_len);
        bar.set_message(f!("Creating: {variant}.mp4 from TS Files"));
        bar.set_style(
            ProgressStyle::default_spinner().template("{spinner}{bar:80.cyan/blue} {percent}% | [{eta_precise}][{elapsed_precise}] ETA/Elapsed | {msg}{pos:>5}/{len:4}").unwrap()
        );

        reader
            .lines()
            .filter_map(|line| line.ok())
            .for_each(|line| if line.contains("Opening") { bar.inc(1) } );

        bar.finish();
        println_f!("\n\n");
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

    let client: Client = Client::new();
    let video_info: VideoInfo = get_video_info(&client, &args).await;
    let playlist_url: String = get_playlist_info(&client, &args).await.playlist_url;
    let playlist_variants: String = web_get_text(&client, &playlist_url).await;

    println_f!(">> Downloading: \"{video_info.title}\"");

    let variants: Vec<&str> = playlist_variants.split("\r\n").collect();

    let mut variant_urls: Vec<String> = vec![];
    let mut variant_names: Vec<&str> = vec![];

    for url in variants {
        if url.starts_with("https") {
            variant_urls.push(String::from(url));
        }
    }

    for variant in &variant_urls {


        let url_parts: Vec<&str> = variant.split("/").collect();
        let quality: &str = url_parts[5];
        let id: &str = { url_parts[4] };
        let ts_base: String = f!("https://01.cdn.vod.farm/transcode/{id}/{quality}/");
        let output: String = f!("./{args.output_path}/{args.vod_id}/{quality}");
        variant_names.push(&quality);
        println_f!("Start processing: {quality}");

        fs::create_dir_all(output).expect("Failed to create output directory!");

        let playlist: String = web_get_text(&client, &variant).await;

        let mut ts_files: Vec<String> = vec![];
        let lines: Vec<&str> = playlist.split("\n").collect();

        for line in lines {
            if line.ends_with(".ts") {
                ts_files.push(String::from(line));
            }
        }

        let playlist_out: String = f!("./{args.output_path}/{args.vod_id}/{quality}/index.m3u8");

        fs::write(playlist_out, &playlist).expect("Failed to write playlist!");

        let ts_files_len = ts_files.len() as u64;

        let bar = ProgressBar::new(ts_files_len.clone());
        bar.set_style(
            ProgressStyle::default_spinner().template("{spinner}{bar:80.cyan/blue} {percent}% | [{eta_precise}][{elapsed_precise}] ETA/Elapsed | {msg}{pos:>5}/{len:4}").unwrap()
        );
        bar.set_message(f!("Downloading: TS Files"));
        for file in ts_files {

            let full_url: String = f!("{ts_base}{file}");
            let output_file: String = f!("./{args.output_path}/{args.vod_id}/{quality}/{file}");

            if Path::new(&output_file).exists() == false {
                let mut out_file: File = File::create(&output_file).unwrap();
                let ts_file = client.get(full_url).send().await.unwrap().bytes().await.unwrap();
                out_file.write_all(&ts_file).expect("Failed to write ts file.");
            }
            bar.inc(1);
        }
        bar.finish();
        let mp4_output: String = f!("./{args.output_path}/{args.vod_id}/{quality}.mp4");

        if &mp4_output != "" && Path::new(&mp4_output).exists() == false {
            hls_to_mp4(&args, &quality, ts_files_len.clone());
        } else {
            println_f!("Skip generating {quality}.mp4. Reason: Output file already exists\n\n");
        }
    }

    let master_playlist = create_master_playlist(&variant_names);
    let master_playlist_output = f!("./{args.output_path}/{args.vod_id}/index.m3u8");
    fs::write(master_playlist_output, master_playlist).expect("Failed to write master playlist");

    println_f!("Downloaded {video_info} to ./{args.output_path}/{args.vod_id}")
}
