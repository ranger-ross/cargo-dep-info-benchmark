#![allow(dead_code)]
use crate::paths;
use serde::Serialize;
use std::{
    fmt,
    path::PathBuf,
    str::{self, FromStr},
};
type CargoResult<T> = anyhow::Result<T>;
/// The current format version of [`EncodedDepInfo`].
const CURRENT_ENCODED_DEP_INFO_VERSION: u8 = 1;
/// Tells the associated path in [`EncodedDepInfo::files`] is relative to package root,
/// target root, or absolute.
#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub enum DepInfoPathType {
    /// src/, e.g. src/lib.rs
    PackageRootRelative,
    /// {build-dir}/debug/deps/lib...
    /// or an absolute path /.../sysroot/...
    BuildRootRelative,
    /// Relative to the workspace root (but not in the package root)
    WorkspaceRootRelative,
}

/// Same as [`RustcDepInfo`] except avoids absolute paths as much as possible to
/// allow moving around the target directory.
///
/// This is also stored in an optimized format to make parsing it fast because
/// Cargo will read it for crates on all future compilations.
///
/// Currently the format looks like:
///
/// ```text
/// +--------+---------+------------+------------+---------------+---------------+
/// | marker | version | # of files | file paths | # of env vars | env var pairs |
/// +--------+---------+------------+------------+---------------+---------------+
/// ```
///
/// Each field represents
///
/// * _Marker_ --- A magic marker to ensure that older Cargoes, which only
///   recognize format v0 (prior to checksum support in [`f4ca7390`]), do not
///   proceed with parsing newer formats. Since [`EncodedDepInfo`] is merely
///   an optimization, and to avoid adding complexity, Cargo recognizes only
///   one version of [`CURRENT_ENCODED_DEP_INFO_VERSION`].
///   The current layout looks like this
///   ```text
///   +----------------------------+
///   | [0x01 0x00 0x00 0x00 0xff] |
///   +----------------------------+
///   ```
///   These bytes will be interpreted as "one file tracked and an invalid
///   [`DepInfoPathType`] variant with 255" by older Cargoes, causing them to
///   stop parsing. This could prevent problematic parsing as noted in
///   rust-lang/cargo#14712.
/// * _Version_ --- The current format version.
/// * _Number of files/envs_ --- A `u32` representing the number of things.
/// * _File paths_ --- Zero or more paths of files the dep-info file depends on.
///   Each path is encoded as the following:
///
///   ```text
///   +-----------+-------------+------------+---------------+-----------+-------+
///   | path type | len of path | path bytes | cksum exists? | file size | cksum |
///   +-----------+-------------+------------+---------------+-----------+-------+
///   ```
/// * _Env var pairs_ --- Zero or more env vars the dep-info file depends on.
///   Each env key-value pair is encoded as the following:
///   ```text
///   +------------+-----------+---------------+--------------+-------------+
///   | len of key | key bytes | value exists? | len of value | value bytes |
///   +------------+-----------+---------------+--------------+-------------+
///   ```
///
/// [`f4ca7390`]: https://github.com/rust-lang/cargo/commit/f4ca739073185ea5e1148ff100bb4a06d3bf721d
#[derive(Default, Debug, PartialEq, Eq)]
pub struct EncodedDepInfo {
    pub files: Vec<(DepInfoPathType, PathBuf, Option<(u64, String)>)>,
    pub env: Vec<(String, Option<String>)>,
}

impl EncodedDepInfo {
    pub fn parse(mut bytes: &[u8]) -> Option<EncodedDepInfo> {
        let bytes = &mut bytes;
        read_magic_marker(bytes)?;
        let version = read_u8(bytes)?;
        if version != CURRENT_ENCODED_DEP_INFO_VERSION {
            return None;
        }

        let nfiles = read_usize(bytes)?;
        let mut files = Vec::with_capacity(nfiles);
        for _ in 0..nfiles {
            let ty = match read_u8(bytes)? {
                0 => DepInfoPathType::PackageRootRelative,
                1 => DepInfoPathType::BuildRootRelative,
                2 => DepInfoPathType::WorkspaceRootRelative,
                _ => return None,
            };
            let path_bytes = read_bytes(bytes)?;
            let path = paths::bytes2path(path_bytes).ok()?;
            let has_checksum = read_bool(bytes)?;
            let checksum_info = has_checksum
                .then(|| {
                    let file_len = read_u64(bytes);
                    let checksum_string = read_bytes(bytes)
                        .map(Vec::from)
                        .and_then(|v| String::from_utf8(v).ok());
                    file_len.zip(checksum_string)
                })
                .flatten();
            files.push((ty, path, checksum_info));
        }

        let nenv = read_usize(bytes)?;
        let mut env = Vec::with_capacity(nenv);
        for _ in 0..nenv {
            let key = str::from_utf8(read_bytes(bytes)?).ok()?.to_string();
            let val = match read_u8(bytes)? {
                0 => None,
                1 => Some(str::from_utf8(read_bytes(bytes)?).ok()?.to_string()),
                _ => return None,
            };
            env.push((key, val));
        }
        return Some(EncodedDepInfo { files, env });

        /// See [`EncodedDepInfo`] for why a magic marker exists.
        fn read_magic_marker(bytes: &mut &[u8]) -> Option<()> {
            let _size = read_usize(bytes)?;
            let path_type = read_u8(bytes)?;
            if path_type != u8::MAX {
                // Old depinfo. Give up parsing it.
                None
            } else {
                Some(())
            }
        }

        fn read_usize(bytes: &mut &[u8]) -> Option<usize> {
            let ret = bytes.get(..4)?;
            *bytes = &bytes[4..];
            Some(u32::from_le_bytes(ret.try_into().unwrap()) as usize)
        }

        fn read_u64(bytes: &mut &[u8]) -> Option<u64> {
            let ret = bytes.get(..8)?;
            *bytes = &bytes[8..];
            Some(u64::from_le_bytes(ret.try_into().unwrap()))
        }

        fn read_bool(bytes: &mut &[u8]) -> Option<bool> {
            read_u8(bytes).map(|b| b != 0)
        }

        fn read_u8(bytes: &mut &[u8]) -> Option<u8> {
            let ret = *bytes.get(0)?;
            *bytes = &bytes[1..];
            Some(ret)
        }

        fn read_bytes<'a>(bytes: &mut &'a [u8]) -> Option<&'a [u8]> {
            let n = read_usize(bytes)? as usize;
            let ret = bytes.get(..n)?;
            *bytes = &bytes[n..];
            Some(ret)
        }
    }

    pub fn serialize(&self) -> CargoResult<Vec<u8>> {
        let mut ret = Vec::new();
        let dst = &mut ret;

        write_magic_marker(dst);
        dst.push(CURRENT_ENCODED_DEP_INFO_VERSION);

        write_usize(dst, self.files.len());
        for (ty, file, checksum_info) in self.files.iter() {
            match ty {
                DepInfoPathType::PackageRootRelative => dst.push(0),
                DepInfoPathType::BuildRootRelative => dst.push(1),
                DepInfoPathType::WorkspaceRootRelative => dst.push(2),
            }
            write_bytes(dst, paths::path2bytes(file)?);
            write_bool(dst, checksum_info.is_some());
            if let Some((len, checksum)) = checksum_info {
                write_u64(dst, *len);
                write_bytes(dst, checksum);
            }
        }

        write_usize(dst, self.env.len());
        for (key, val) in self.env.iter() {
            write_bytes(dst, key);
            match val {
                None => dst.push(0),
                Some(val) => {
                    dst.push(1);
                    write_bytes(dst, val);
                }
            }
        }
        return Ok(ret);

        /// See [`EncodedDepInfo`] for why a magic marker exists.
        ///
        /// There is an assumption that there is always at least a file.
        fn write_magic_marker(dst: &mut Vec<u8>) {
            write_usize(dst, 1);
            dst.push(u8::MAX);
        }

        fn write_bytes(dst: &mut Vec<u8>, val: impl AsRef<[u8]>) {
            let val = val.as_ref();
            write_usize(dst, val.len());
            dst.extend_from_slice(val);
        }

        fn write_usize(dst: &mut Vec<u8>, val: usize) {
            dst.extend(&u32::to_le_bytes(val as u32));
        }

        fn write_u64(dst: &mut Vec<u8>, val: u64) {
            dst.extend(&u64::to_le_bytes(val));
        }

        fn write_bool(dst: &mut Vec<u8>, val: bool) {
            dst.push(u8::from(val));
        }
    }
}
/// Some algorithms are here to ensure compatibility with possible rustc outputs.
/// The presence of an algorithm here is not a suggestion that it's fit for use.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ChecksumAlgo {
    Sha256,
    Blake3,
}

impl ChecksumAlgo {
    fn hash_len(&self) -> usize {
        match self {
            ChecksumAlgo::Sha256 | ChecksumAlgo::Blake3 => 32,
        }
    }
}

impl FromStr for ChecksumAlgo {
    type Err = InvalidChecksum;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sha256" => Ok(Self::Sha256),
            "blake3" => Ok(Self::Blake3),
            _ => Err(InvalidChecksum::InvalidChecksumAlgo),
        }
    }
}

impl fmt::Display for ChecksumAlgo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChecksumAlgo::Sha256 => "sha256",
            ChecksumAlgo::Blake3 => "blake3",
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Checksum {
    algo: ChecksumAlgo,
    /// If the algorithm uses fewer than 32 bytes, then the remaining bytes will be zero.
    value: [u8; 32],
}
impl FromStr for Checksum {
    type Err = InvalidChecksum;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('=');
        let Some(algo) = parts.next().map(ChecksumAlgo::from_str).transpose()? else {
            return Err(InvalidChecksum::InvalidFormat);
        };
        let Some(checksum) = parts.next() else {
            return Err(InvalidChecksum::InvalidFormat);
        };
        let mut value = [0; 32];
        if hex::decode_to_slice(checksum, &mut value[0..algo.hash_len()]).is_err() {
            return Err(InvalidChecksum::InvalidChecksum(algo));
        }
        Ok(Self { algo, value })
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut checksum = [0; 64];
        let hash_len = self.algo.hash_len();
        hex::encode_to_slice(&self.value[0..hash_len], &mut checksum[0..(hash_len * 2)])
            .map_err(|_| fmt::Error)?;
        write!(
            f,
            "{}={}",
            self.algo,
            str::from_utf8(&checksum[0..(hash_len * 2)]).unwrap_or_default()
        )
    }
}

impl Serialize for Checksum {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Checksum {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidChecksum {
    #[error("algorithm portion incorrect, expected `sha256`, or `blake3`")]
    InvalidChecksumAlgo,
    #[error("expected {} hexadecimal digits in checksum portion", .0.hash_len() * 2)]
    InvalidChecksum(ChecksumAlgo),
    #[error("expected a string with format \"algorithm=hex_checksum\"")]
    InvalidFormat,
}
