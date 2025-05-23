use std::collections::HashSet;

use super::{Vendor, normalize_architecture, normalize_os, normalize_version};
use crate::{
    github::{self, GitHubAsset, GitHubRelease},
    http::HTTP,
    jvm::JvmData,
};
use eyre::Result;
use log::{debug, warn};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use xx::regex;

#[derive(Clone, Copy, Debug)]
pub struct GraalVM {}

#[derive(Debug, PartialEq)]
struct FileNameMeta {
    arch: String,
    ext: String,
    java_version: String,
    os: String,
    version: String,
}

impl Vendor for GraalVM {
    fn get_name(&self) -> String {
        "graalvm".to_string()
    }

    fn fetch_data(&self, jvm_data: &mut HashSet<JvmData>) -> Result<()> {
        let releases = github::list_releases("graalvm/graalvm-ce-builds")?;
        let data = releases
            .into_par_iter()
            .flat_map(|release| {
                map_release(&release).unwrap_or_else(|err| {
                    warn!("[graalvm] error parsing release: {}", err);
                    vec![]
                })
            })
            .collect::<Vec<JvmData>>();
        jvm_data.extend(data);
        Ok(())
    }
}

fn map_release(release: &GitHubRelease) -> Result<Vec<JvmData>> {
    let assets = release
        .assets
        .iter()
        .filter(|asset| include(asset))
        .collect::<Vec<&GitHubAsset>>();

    let jvm_data = assets
        .into_par_iter()
        .filter_map(|asset| match map_asset(asset) {
            Ok(meta) => Some(meta),
            Err(e) => {
                warn!("[graalvm] {}", e);
                None
            }
        })
        .collect::<Vec<JvmData>>();

    Ok(jvm_data)
}

fn map_asset(asset: &GitHubAsset) -> Result<JvmData> {
    if asset.name.starts_with("graalvm-ce") {
        map_ce(asset)
    } else if asset.name.starts_with("graalvm-community") {
        map_community(asset)
    } else {
        Err(eyre::eyre!("unknown asset: {}", asset.name))
    }
}

fn map_ce(asset: &GitHubAsset) -> Result<JvmData> {
    let sha256_url = format!("{}.sha256", asset.browser_download_url);
    let sha256 = match HTTP.get_text(&sha256_url) {
        Ok(sha256) => Some(format!("sha256:{}", sha256.trim())),
        Err(_) => {
            warn!("[graalvm] unable to find SHA256 for {}", asset.name);
            None
        }
    };
    let filename = asset.name.clone();
    let filename_meta = meta_from_name_ce(&filename)?;
    let url = asset.browser_download_url.clone();
    let version = normalize_version(&filename_meta.version);
    Ok(JvmData {
        architecture: normalize_architecture(&filename_meta.arch),
        checksum: sha256,
        checksum_url: Some(sha256_url.clone()),
        filename,
        file_type: filename_meta.ext.clone(),
        image_type: "jdk".to_string(),
        java_version: filename_meta.java_version.clone(),
        jvm_impl: "graalvm".to_string(),
        os: normalize_os(&filename_meta.os),
        release_type: "ga".to_string(),
        url,
        vendor: "graalvm".to_string(),
        version: format!("{}+java{}", version, filename_meta.java_version.clone()),
        ..Default::default()
    })
}

fn map_community(asset: &GitHubAsset) -> Result<JvmData> {
    let sha256_url = format!("{}.sha256", asset.browser_download_url);
    let sha256sum = match HTTP.get_text(&sha256_url) {
        Ok(sha256) => Some(format!("sha256:{}", sha256)),
        Err(_) => {
            warn!("[graalvm] unable to find SHA256 for asset: {}", asset.name);
            None
        }
    };
    let filename = asset.name.clone();
    let filename_meta = meta_from_name_community(&filename)?;
    let url = asset.browser_download_url.clone();
    let version = normalize_version(&filename_meta.version);
    Ok(JvmData {
        architecture: normalize_architecture(&filename_meta.arch),
        checksum: sha256sum,
        checksum_url: Some(sha256_url),
        filename,
        file_type: filename_meta.ext.clone(),
        image_type: "jdk".to_string(),
        java_version: version.clone(),
        jvm_impl: "graalvm".to_string(),
        os: normalize_os(&filename_meta.os),
        release_type: "ga".to_string(),
        url,
        vendor: "graalvm-community".to_string(),
        version,
        ..Default::default()
    })
}

fn include(asset: &GitHubAsset) -> bool {
    (asset.name.starts_with("graalvm-ce") || asset.name.starts_with("graalvm-community"))
        && (asset.name.ends_with(".tar.gz") || asset.name.ends_with(".zip"))
}

fn meta_from_name_ce(name: &str) -> Result<FileNameMeta> {
    debug!("[graalvm] parsing name: {}", name);
    let capture = regex!(r"^graalvm-ce-(?:complete-)?java([0-9]{1,2})-(linux|darwin|windows)-(aarch64|amd64)-([0-9+.]{2,})\.(zip|tar\.gz)$")
        .captures(name)
        .ok_or_else(|| eyre::eyre!("regular expression did not match name: {}", name))?;

    let java_version = capture.get(1).unwrap().as_str().to_string();
    let os = capture.get(2).unwrap().as_str().to_string();
    let arch = capture.get(3).unwrap().as_str().to_string();
    let version = capture.get(4).unwrap().as_str().to_string();
    let ext = capture.get(5).unwrap().as_str().to_string();

    Ok(FileNameMeta {
        arch,
        ext,
        java_version,
        os,
        version,
    })
}

fn meta_from_name_community(name: &str) -> Result<FileNameMeta> {
    debug!("[graalvm] parsing name: {}", name);
    let capture = regex!(r"^graalvm-community-jdk-([0-9]{1,2}\.[0-9]{1}\.[0-9]{1,3})_(linux|macos|windows)-(aarch64|x64)_bin\.(zip|tar\.gz)$")
      .captures(name)
      .ok_or_else(|| eyre::eyre!("regular expression did not match name: {}", name))?;

    let java_version = capture.get(1).unwrap().as_str().to_string();
    let os = capture.get(2).unwrap().as_str().to_string();
    let arch = capture.get(3).unwrap().as_str().to_string();
    let ext = capture.get(4).unwrap().as_str().to_string();

    Ok(FileNameMeta {
        arch,
        ext,
        java_version: java_version.clone(),
        os,
        version: java_version.clone(),
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_meta_from_name_ce() {
        for (actual, expected) in [
            (
                "graalvm-ce-java8-windows-amd64-19.3.4.zip",
                FileNameMeta {
                    arch: "amd64".to_string(),
                    ext: "zip".to_string(),
                    java_version: "8".to_string(),
                    os: "windows".to_string(),
                    version: "19.3.4".to_string(),
                },
            ),
            (
                "graalvm-ce-java11-darwin-aarch64-22.3.0.tar.gz",
                FileNameMeta {
                    arch: "aarch64".to_string(),
                    ext: "tar.gz".to_string(),
                    java_version: "11".to_string(),
                    os: "darwin".to_string(),
                    version: "22.3.0".to_string(),
                },
            ),
        ] {
            assert_eq!(meta_from_name_ce(actual).unwrap(), expected);
        }
    }

    #[test]
    fn test_meta_from_name_community() {
        for (actual, expected) in [
            (
                "graalvm-community-jdk-17.0.8_linux-aarch64_bin.tar.gz",
                FileNameMeta {
                    arch: "aarch64".to_string(),
                    ext: "tar.gz".to_string(),
                    java_version: "17.0.8".to_string(),
                    os: "linux".to_string(),
                    version: "17.0.8".to_string(),
                },
            ),
            (
                "graalvm-community-jdk-23.0.2_windows-x64_bin.zip",
                FileNameMeta {
                    arch: "x64".to_string(),
                    ext: "zip".to_string(),
                    java_version: "23.0.2".to_string(),
                    os: "windows".to_string(),
                    version: "23.0.2".to_string(),
                },
            ),
        ] {
            assert_eq!(meta_from_name_community(actual).unwrap(), expected);
        }
    }
}
