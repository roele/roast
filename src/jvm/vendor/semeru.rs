use super::{Vendor, normalize_architecture, normalize_os, normalize_version};
use crate::{
    github::{self, GitHubAsset, GitHubRelease},
    http::HTTP,
    jvm::JvmData,
};
use eyre::Result;
use log::{debug, warn};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::HashSet;
use xx::regex;

#[derive(Clone, Copy, Debug)]
pub struct Semeru {}

#[derive(Debug, PartialEq)]
struct FileNameMeta {
    arch: String,
    image_type: String,
    os: String,
    ext: String,
}

impl Vendor for Semeru {
    fn get_name(&self) -> String {
        "semeru".to_string()
    }

    fn fetch_data(&self, jvm_data: &mut HashSet<JvmData>) -> Result<()> {
        for version in &[
            "8",
            "11",
            "11-certified",
            "16",
            "17",
            "17-certified",
            "18",
            "19",
            "20",
            "21",
            "21-certified",
            "22",
            "23",
        ] {
            debug!("[semeru] fetching releases for version: {version}");

            let slug = format!("ibmruntimes/semeru{version}-binaries");
            let releases = github::list_releases(slug.as_str())?;
            let data = releases
                .into_par_iter()
                .filter(|release| !release.prerelease)
                .flat_map(|release| {
                    map_release(&release).unwrap_or_else(|err| {
                        warn!("[semeru] failed to map release: {}", err);
                        vec![]
                    })
                })
                .collect::<Vec<JvmData>>();
            jvm_data.extend(data);
        }
        Ok(())
    }
}

fn map_release(release: &GitHubRelease) -> Result<Vec<JvmData>> {
    let assets = release
        .assets
        .iter()
        .filter(|asset| include(asset))
        .collect::<Vec<&github::GitHubAsset>>();

    let jvm_data = assets
        .into_par_iter()
        .filter_map(|asset| match map_asset(release, asset) {
            Ok(meta) => Some(meta),
            Err(e) => {
                warn!("[semeru] {}", e);
                None
            }
        })
        .collect::<Vec<JvmData>>();

    Ok(jvm_data)
}

fn include(asset: &github::GitHubAsset) -> bool {
    (asset.name.ends_with(".zip")
        || asset.name.ends_with(".tar.gz")
        || asset.name.ends_with(".msi")
        || asset.name.ends_with(".rpm"))
        && !asset.name.ends_with(".tap.zip")
        && !asset.name.contains("debugimage")
        && !asset.name.contains("testimage")
}

fn map_asset(release: &GitHubRelease, asset: &GitHubAsset) -> Result<JvmData> {
    let sha256_url = format!("{}.sha256.txt", asset.browser_download_url);
    let sha256 = match HTTP.get_text(&sha256_url) {
        Ok(sha256) => match sha256.split_whitespace().next() {
            Some(sha256) => Some(format!("sha256:{}", sha256.trim())),
            None => {
                warn!("[semeru] unable to parse SHA256 for {}", asset.name);
                None
            }
        },
        Err(_) => {
            warn!("[semeru] unable to find SHA256 for {}", asset.name);
            None
        }
    };
    let filename = asset.name.clone();
    let filename_meta = meta_from_name(&filename)?;
    let url = asset.browser_download_url.clone();
    let version = version_from_tag(&release.tag_name)?;
    Ok(JvmData {
        architecture: normalize_architecture(&filename_meta.arch),
        checksum: sha256,
        checksum_url: Some(sha256_url),
        features: if asset.name.contains("-certified") {
            Some(vec!["certified".to_string()])
        } else {
            None
        },
        filename,
        file_type: filename_meta.ext.clone(),
        image_type: filename_meta.image_type.clone(),
        java_version: normalize_version(&version),
        jvm_impl: "openj9".to_string(),
        os: normalize_os(&filename_meta.os),
        release_type: "ga".to_string(),
        url,
        vendor: "semeru".to_string(),
        version: normalize_version(&version),
        ..Default::default()
    })
}

fn version_from_tag(tag: &str) -> Result<String> {
    let capture = regex!(r"^(?:jdk-?)?(.*)[_-]openj9-(.*)$")
        .captures(tag)
        .ok_or_else(|| eyre::eyre!("regular expression failed for tag: {}", tag))?;
    let version = capture.get(1).unwrap().as_str().to_string();
    let openj_version = capture.get(2).unwrap().as_str().to_string();
    Ok(format!("{version}_openj9-{openj_version}"))
}

fn meta_from_name(name: &str) -> Result<FileNameMeta> {
    debug!("[semeru] parsing name: {}", name);
    match name {
        name if name.ends_with(".rpm") => meta_from_name_rpm(name),
        _ => meta_from_name_other(name),
    }
}

fn meta_from_name_other(name: &str) -> Result<FileNameMeta> {
    let capture = regex!(r"^ibm-semeru-(?:open|certified)-(jre|jdk)_(x64|x86-32|x86-64|x86_64|s390x|ppc64|ppc64le|aarch64)_(aix|linux|mac|windows)_(?:.+_openj9-)?.+\.(tar\.gz|zip|msi)$")
        .captures(name)
        .ok_or_else(|| eyre::eyre!("regular expression failed for name: {}", name))?;

    let image_type = capture.get(1).unwrap().as_str().to_string();
    let arch = capture.get(2).unwrap().as_str().to_string();
    let os = capture.get(3).unwrap().as_str().to_string();
    let ext = capture.get(4).unwrap().as_str().to_string();

    Ok(FileNameMeta {
        arch,
        image_type,
        os,
        ext,
    })
}

fn meta_from_name_rpm(name: &str) -> Result<FileNameMeta> {
    let capture =
        regex!(r"^ibm-semeru-(?:open|certified)-[0-9]+-(jre|jdk)-(.+)\.(x86_64|s390x|ppc64|ppc64le|aarch64)\.rpm$")
            .captures(name)
            .ok_or_else(|| eyre::eyre!("regular expression failed for name: {}", name))?;

    let os = "linux".to_string();
    let image_type = capture.get(1).unwrap().as_str().to_string();
    let ext = "rpm".to_string();
    let arch = capture.get(3).unwrap().as_str().to_string();

    Ok(FileNameMeta {
        arch,
        image_type,
        os,
        ext,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_meta_from_name() {
        for (actual, expected) in [
            (
                "ibm-semeru-certified-jdk_aarch64_linux_11.0.20.0.tar.gz",
                FileNameMeta {
                    arch: "aarch64".to_string(),
                    ext: "tar.gz".to_string(),
                    image_type: "jdk".to_string(),
                    os: "linux".to_string(),
                },
            ),
            (
                "ibm-semeru-open-jdk_aarch64_mac_17.0.11_9_openj9-0.44.0.tar.gz",
                FileNameMeta {
                    arch: "aarch64".to_string(),
                    ext: "tar.gz".to_string(),
                    image_type: "jdk".to_string(),
                    os: "mac".to_string(),
                },
            ),
            (
                "ibm-semeru-open-jdk_x64_windows_11.0.22_7_openj9-0.43.0.zip",
                FileNameMeta {
                    arch: "x64".to_string(),
                    ext: "zip".to_string(),
                    image_type: "jdk".to_string(),
                    os: "windows".to_string(),
                },
            ),
        ] {
            assert_eq!(meta_from_name(actual).unwrap(), expected);
        }
    }

    #[test]
    fn test_meta_from_name_rpm() {
        for (actual, expected) in [
            (
                "ibm-semeru-open-17-jdk-17.0.13.11_0.48.0-1.aarch64.rpm",
                FileNameMeta {
                    arch: "aarch64".to_string(),
                    ext: "rpm".to_string(),
                    image_type: "jdk".to_string(),
                    os: "linux".to_string(),
                },
            ),
            (
                "ibm-semeru-certified-11-jdk-11.0.25.0-1.x86_64.rpm",
                FileNameMeta {
                    arch: "x86_64".to_string(),
                    ext: "rpm".to_string(),
                    image_type: "jdk".to_string(),
                    os: "linux".to_string(),
                },
            ),
        ] {
            assert_eq!(meta_from_name(actual).unwrap(), expected);
        }
    }
}
