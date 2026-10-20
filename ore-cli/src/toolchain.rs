//! Managed Zig toolchain: `ore mktool` uses Zig to compile C, C++, and Zig to WebAssembly.
//!
//! Resolution order (`ensure_zig`):
//!   1. `ORE_ZIG` environment variable (path to a zig binary)
//!   2. `zig` on PATH
//!   3. The ORE-managed install in `<ORE_DIR>/toolchains/zig-<version>/`
//!   4. Otherwise, download and install it (`install_zig`)
//!
//! Downloads follow ziglang.org's guidance for automated tools: try the community
//! mirrors in random order (ziglang.org itself last), verify every archive's minisign
//! signature against the Zig Software Foundation key, and check that the signed
//! file name matches the requested archive (prevents downgrade attacks).

use crate::utils::get_ore_dir;
use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use minisign_verify::{PublicKey, Signature};
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Pinned Zig release: the version the ORE SDK (`ore.zig`) and `mktool` flags are tested with.
pub const ZIG_VERSION: &str = "0.17.0";

/// Zig Software Foundation minisign public key (https://ziglang.org/download/).
const ZIG_PUBLIC_KEY: &str = "RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U";

const MIRROR_LIST_URL: &str = "https://ziglang.org/download/community-mirrors.txt";

/// Identifies this tool to mirrors, as requested by ziglang.org.
const SOURCE_TAG: &str = "ore-cli";

/// Refuse anything larger than this (current archives are ~50-90 MB).
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

/// Zig's (arch, os) names for the current host, e.g. ("x86_64", "windows").
fn zig_host() -> Option<(&'static str, &'static str)> {
    let arch = match std::env::consts::ARCH {
        a @ ("x86_64" | "aarch64" | "x86" | "arm" | "riscv64" | "powerpc64le" | "loongarch64"
        | "s390x") => a,
        _ => return None,
    };
    let os = match std::env::consts::OS {
        o @ ("windows" | "linux" | "macos" | "freebsd" | "netbsd") => o,
        _ => return None,
    };
    Some((arch, os))
}

/// Release archive name, e.g. `zig-x86_64-windows-0.17.0.zip`.
fn archive_name(arch: &str, os: &str, version: &str) -> String {
    let ext = if os == "windows" { "zip" } else { "tar.xz" };
    format!("zig-{}-{}-{}.{}", arch, os, version, ext)
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "zig.exe" } else { "zig" }
}

fn toolchains_dir() -> PathBuf {
    get_ore_dir().join("toolchains")
}

/// Where the managed Zig lives: `<ORE_DIR>/toolchains/zig-<version>/zig(.exe)`.
pub fn managed_zig_path() -> PathBuf {
    toolchains_dir()
        .join(format!("zig-{}", ZIG_VERSION))
        .join(exe_name())
}

/// Runs `<zig> version`; returns the version string if the binary works.
fn zig_version(zig: &Path) -> Option<String> {
    let out = Command::new(zig).arg("version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Returns a working Zig binary, installing the managed toolchain if none is available.
pub async fn ensure_zig() -> PathBuf {
    if let Ok(custom) = std::env::var("ORE_ZIG") {
        let custom = PathBuf::from(custom);
        if zig_version(&custom).is_some() {
            return custom;
        }
        println!(
            "{} ORE_ZIG is set to '{}', but it isn't a working zig binary. Ignoring it.",
            "[!]".yellow(),
            custom.display()
        );
    }

    let on_path = PathBuf::from("zig");
    if zig_version(&on_path).is_some() {
        return on_path;
    }

    let managed = managed_zig_path();
    if zig_version(&managed).is_some() {
        return managed;
    }

    println!(
        "{} Zig not found. ORE uses Zig to compile C, C++, and Zig to WebAssembly.",
        "[*]".bright_blue()
    );
    match install_zig().await {
        Ok(path) => path,
        Err(e) => {
            print_install_failure(&e);
            std::process::exit(1);
        }
    }
}

fn print_install_failure(err: &str) {
    println!("{} Failed to install Zig: {}", "[-]".red().bold(), err);
    println!(
        "    {} Install Zig {} manually from https://ziglang.org/download/ and put it on your PATH,",
        "[i]".bright_blue(),
        ZIG_VERSION
    );
    println!("        or point ORE_ZIG at the zig binary. Then run the command again.");
}

/// Downloads, verifies, and installs the pinned Zig into `<ORE_DIR>/toolchains/`.
/// Returns the path of the installed binary. Safe to run again: an existing
/// working install is reused.
pub async fn install_zig() -> Result<PathBuf, String> {
    let managed = managed_zig_path();
    if let Some(v) = zig_version(&managed) {
        println!(
            "{} Zig {} is already installed at {}",
            "[+]".green(),
            v,
            managed.display()
        );
        return Ok(managed);
    }

    let (arch, os) = zig_host().ok_or_else(|| {
        format!(
            "no prebuilt Zig for this platform ({} {})",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let archive = archive_name(arch, os, ZIG_VERSION);

    let tc_dir = toolchains_dir();
    fs::create_dir_all(&tc_dir)
        .map_err(|e| format!("cannot create {}: {}", tc_dir.display(), e))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!("ore-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("cannot build HTTP client: {}", e))?;

    let public_key =
        PublicKey::from_base64(ZIG_PUBLIC_KEY).map_err(|e| format!("bad Zig public key: {}", e))?;

    // Mirrors in random order, then ziglang.org as the last resort.
    let mut sources = fetch_mirrors(&client).await;
    shuffle(&mut sources);
    sources.push(format!("https://ziglang.org/download/{}", ZIG_VERSION));

    let archive_path = tc_dir.join(format!(".download-{}", archive));
    let mut verified = false;

    println!(
        "{} Installing Zig {} ({}) into {}",
        "[*]".bright_blue(),
        ZIG_VERSION,
        archive,
        tc_dir.display()
    );

    for base in &sources {
        let host = base.split('/').nth(2).unwrap_or(base);
        println!("{} Trying {}...", "[~]".yellow(), host);

        match download_and_verify(&client, base, &archive, &archive_path, &public_key).await {
            Ok(()) => {
                println!(
                    "{} Signature verified (Zig Software Foundation key).",
                    "[+]".green()
                );
                verified = true;
                break;
            }
            Err(e) => {
                println!("    {} {}", "[!]".yellow(), e);
                let _ = fs::remove_file(&archive_path);
            }
        }
    }

    if !verified {
        return Err("no mirror returned a correctly signed archive".to_string());
    }

    println!("{} Extracting...", "[~]".yellow());
    let staging = tc_dir.join(format!(".staging-zig-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let extracted = extract_archive(&archive_path, &staging);
    let _ = fs::remove_file(&archive_path);
    extracted.map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        format!("extraction failed: {}", e)
    })?;

    // The archive holds a single top-level folder (zig-<arch>-<os>-<version>/).
    let root = single_subdir(&staging).unwrap_or_else(|| staging.clone());
    let final_dir = tc_dir.join(format!("zig-{}", ZIG_VERSION));
    let _ = fs::remove_dir_all(&final_dir);
    fs::rename(&root, &final_dir).map_err(|e| format!("cannot move Zig into place: {}", e))?;
    let _ = fs::remove_dir_all(&staging);

    let installed = final_dir.join(exe_name());
    match zig_version(&installed) {
        Some(v) => {
            println!(
                "{} Zig {} installed at {}",
                "[+]".green().bold(),
                v,
                installed.display()
            );
            Ok(installed)
        }
        None => Err(format!(
            "installed binary at {} does not run",
            installed.display()
        )),
    }
}

/// Fetches the community mirror list. An empty list just means "use ziglang.org".
async fn fetch_mirrors(client: &reqwest::Client) -> Vec<String> {
    let text = match client.get(MIRROR_LIST_URL).send().await {
        Ok(res) if res.status().is_success() => res.text().await.unwrap_or_default(),
        _ => String::new(),
    };
    parse_mirror_list(&text)
}

fn parse_mirror_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| l.starts_with("https://"))
        .map(|l| l.trim_end_matches('/').to_string())
        .collect()
}

/// Fisher-Yates shuffle seeded from the clock (good enough to spread load across mirrors).
fn shuffle(items: &mut [String]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        ^ ((std::process::id() as u64) << 32);
    for i in (1..items.len()).rev() {
        // xorshift64
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// Downloads `<base>/<archive>` and its `.minisig`, then verifies both the signature
/// and the signed file name. Leaves the verified archive at `dest`.
async fn download_and_verify(
    client: &reqwest::Client,
    base: &str,
    archive: &str,
    dest: &Path,
    public_key: &PublicKey,
) -> Result<(), String> {
    let sig_url = format!("{}/{}.minisig?source={}", base, archive, SOURCE_TAG);
    let sig_text = client
        .get(&sig_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("signature download failed: {}", e))?
        .text()
        .await
        .map_err(|e| format!("signature download failed: {}", e))?;
    let signature =
        Signature::decode(&sig_text).map_err(|e| format!("malformed signature: {}", e))?;

    check_signed_file_name(signature.trusted_comment(), archive)?;

    let url = format!("{}/{}?source={}", base, archive, SOURCE_TAG);
    download_to_file(client, &url, dest).await?;

    // Hash the archive in chunks rather than loading it all into memory.
    let mut verifier = public_key
        .verify_stream(&signature)
        .map_err(|e| format!("signature rejected: {}", e))?;
    let mut file = fs::File::open(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        verifier.update(&buf[..n]);
    }
    verifier
        .finalize()
        .map_err(|e| format!("signature verification FAILED: {}", e))
}

/// The trusted (signed) comment looks like `timestamp:…\tfile:zig-…\thashed`.
/// Its `file:` field must name exactly the archive we asked for, so a mirror can't
/// serve a different (e.g. older, vulnerable) archive with that file's valid signature.
fn check_signed_file_name(trusted_comment: &str, archive: &str) -> Result<(), String> {
    let signed = trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("file:"))
        .ok_or("signature has no file name")?;
    if signed == archive {
        Ok(())
    } else {
        Err(format!(
            "signature is for '{}', not '{}' (possible downgrade attack)",
            signed, archive
        ))
    }
}

async fn download_to_file(client: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    let res = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("download failed: {}", e))?;

    let total = res.content_length().unwrap_or(0);
    if total > MAX_ARCHIVE_BYTES {
        return Err(format!("archive too large ({} bytes)", total));
    }

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green}[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes:>7}/{total_bytes:7} ({bytes_per_sec}, ETA: {eta})")
            .unwrap()
            .progress_chars("=>-"),
    );

    let mut file = BufWriter::new(fs::File::create(dest).map_err(|e| e.to_string())?);
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download interrupted: {}", e))?;
        downloaded += chunk.len() as u64;
        if downloaded > MAX_ARCHIVE_BYTES {
            pb.finish_and_clear();
            return Err("archive too large".to_string());
        }
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        pb.set_position(downloaded);
    }
    file.flush().map_err(|e| e.to_string())?;
    pb.finish_and_clear();
    Ok(())
}

/// Extracts a `.zip` or `.tar.xz` archive into `dest`.
fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.xz") {
        extract_tar_xz(archive, dest)
    } else {
        Err(format!("unsupported archive format: {}", name))
    }
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name() rejects absolute paths and `..` (zip-slip)
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f = fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

fn extract_tar_xz(archive: &Path, dest: &Path) -> Result<(), String> {
    // Decompress to a temporary .tar on disk (the unpacked Zig is a few hundred MB).
    let tar_path = archive.with_extension("tmp.tar");
    {
        let mut input = BufReader::new(fs::File::open(archive).map_err(|e| e.to_string())?);
        let mut output = BufWriter::new(fs::File::create(&tar_path).map_err(|e| e.to_string())?);
        lzma_rs::xz_decompress(&mut input, &mut output)
            .map_err(|e| format!("xz decompression failed: {}", e))?;
        output.flush().map_err(|e| e.to_string())?;
    }
    let result = (|| {
        let file = fs::File::open(&tar_path).map_err(|e| e.to_string())?;
        // `unpack` refuses entries that would escape `dest` and keeps unix permissions.
        tar::Archive::new(BufReader::new(file))
            .unpack(dest)
            .map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(&tar_path);
    result
}

/// If `dir` contains exactly one directory (and nothing else), returns it.
fn single_subdir(dir: &Path) -> Option<PathBuf> {
    let mut entries = fs::read_dir(dir).ok()?.flatten();
    let first = entries.next()?;
    if entries.next().is_some() || !first.path().is_dir() {
        return None;
    }
    Some(first.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_names_match_ziglang_org() {
        assert_eq!(
            archive_name("x86_64", "windows", "0.17.0"),
            "zig-x86_64-windows-0.17.0.zip"
        );
        assert_eq!(
            archive_name("aarch64", "macos", "0.17.0"),
            "zig-aarch64-macos-0.17.0.tar.xz"
        );
        assert_eq!(
            archive_name("x86_64", "linux", "0.17.0"),
            "zig-x86_64-linux-0.17.0.tar.xz"
        );
    }

    #[test]
    fn current_host_is_supported() {
        assert!(zig_host().is_some());
    }

    #[test]
    fn mirror_list_parsing() {
        let text =
            "# comment\nhttps://a.example/zig/\n\nhttp://insecure.example\n  https://b.example  \n";
        assert_eq!(
            parse_mirror_list(text),
            vec!["https://a.example/zig", "https://b.example"]
        );
    }

    #[test]
    fn shuffle_keeps_all_items() {
        let mut items: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_by_key(|s| s.parse::<u32>().unwrap());
        assert_eq!(sorted, (0..20).map(|i| i.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn signed_file_name_must_match() {
        let tc = "timestamp:1759300000\tfile:zig-x86_64-linux-0.17.0.tar.xz\thashed";
        assert!(check_signed_file_name(tc, "zig-x86_64-linux-0.17.0.tar.xz").is_ok());
        // A valid signature for a different (e.g. older) archive must be rejected.
        assert!(check_signed_file_name(tc, "zig-x86_64-linux-0.18.0.tar.xz").is_err());
        assert!(check_signed_file_name("timestamp:1", "zig-x.tar.xz").is_err());
    }

    #[test]
    fn public_key_parses() {
        assert!(PublicKey::from_base64(ZIG_PUBLIC_KEY).is_ok());
    }

    #[test]
    fn tar_xz_round_trip() {
        let dir = std::env::temp_dir().join(format!("ore_zig_tarxz_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Build zig-test/zig + zig-test/lib/std.txt as a .tar, then xz it.
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (path, data) in [
                ("zig-test/zig", b"bin".as_slice()),
                ("zig-test/lib/std.txt", b"std"),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, path, data).unwrap();
            }
            builder.finish().unwrap();
        }
        let archive = dir.join("zig-test.tar.xz");
        let mut xz = Vec::new();
        lzma_rs::xz_compress(&mut tar_bytes.as_slice(), &mut xz).unwrap();
        fs::write(&archive, xz).unwrap();

        let out = dir.join("out");
        extract_archive(&archive, &out).unwrap();
        let root = single_subdir(&out).unwrap();
        assert_eq!(fs::read(root.join("zig")).unwrap(), b"bin");
        assert_eq!(fs::read(root.join("lib").join("std.txt")).unwrap(), b"std");

        let _ = fs::remove_dir_all(&dir);
    }
}
