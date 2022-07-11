use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;

/// Gronkh.TV VOD Downloader
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// VOD ID
    #[clap(long, value_parser)]
    vod_id: String
}

#[derive(Serialize,Deserialize, Debug)]
#[serde()]
struct PlaylistInfo {
    playlist_url: String
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();

    let mut gtv_playlist_url: String = "https://api.gronkh.tv/v1/video/playlist?episode=".to_owned();
    gtv_playlist_url.push_str(&args.vod_id);

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

    let playlist_variants_request  = client.get(&playlist_url)
        .send()
        .await
        .unwrap()
        .text().await.unwrap();

    let variants: Vec<&str> = playlist_variants_request.split("\r\n").collect();

    let mut variant_urls: Vec<String> = vec![];

    for url in variants {
        if url.starts_with("https") {
            variant_urls.push(String::from(url));
        }
    }

    for variant in &variant_urls {
        let url_parts: Vec<&str> = variant.split("/").collect();
        let quality = url_parts[5];
        let mut ts_base : String  = "https://01.cdn.vod.farm/transcode/".to_owned();
        ts_base.push_str(url_parts[4]);
        ts_base.push_str("/");
        ts_base.push_str(quality);
        ts_base.push_str("/");
        let mut output = "gronkhtv/".to_owned();
        output.push_str(&args.vod_id);
        output.push_str(quality);

        fs::create_dir_all(output).expect("Failed to create output directory!");

        let playlist: String  = client.get(variant)
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

        let mut playlist_out = "gronkhtv/".to_owned();
        playlist_out.push_str(&args.vod_id);
        playlist_out.push_str(&quality);
        playlist_out.push_str("/index.m3u8");

        fs::write(playlist_out, &playlist).expect("Failed to write playlist!");

        for file in ts_files {
            let mut full_url = ts_base.clone();
            full_url.push_str(&file);

            let mut output_file = "gronkhtv/".to_owned();
            output_file.push_str(&args.vod_id);
            output_file.push_str("/");
            output_file.push_str(&quality);
            output_file.push_str("/");
            output_file.push_str(&file);

            let mut out_file = std::fs::File::create(&output_file).unwrap();

            let ts_file = client.get(full_url).send().await.unwrap().bytes().await.unwrap();
            out_file.write_all(&ts_file).expect("Failed to write ts file.");
            println!("Downloaded ./gronkhtv/{}/{}/{}", args.vod_id, quality, file);
        }
    }
}
