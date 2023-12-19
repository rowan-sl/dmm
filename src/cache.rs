use std::{hash::Hash as _, path::PathBuf, str::FromStr};

use base64::Engine;
use highway::{HighwayHash, HighwayHasher};

use crate::schema::Source;

#[derive(Default)]
pub struct CacheDir {
    dir: PathBuf,
}

impl CacheDir {
    pub fn new(path: PathBuf) -> Self {
        Self { dir: path }
    }

    pub fn find(&self, hash: Hash) -> Option<PathBuf> {
        let p = self.dir.join(hash.to_string());
        p.exists().then_some(p)
    }

    pub fn create(&self, hash: Hash) -> PathBuf {
        self.dir.join(hash.to_string())
    }
}

/// Hash of source + input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash {
    hash: [u8; 32],
}

impl Hash {
    pub fn generate(source: &Source, input: &ron::Value) -> Self {
        let mut hasher = HighwayHasher::default();
        // ignore the name of the source, only the input and kind (if the name changes, it wont need to update)
        let Source {
            name: _,
            format,
            kind,
        } = source;
        format.hash(&mut hasher);
        kind.hash(&mut hasher);
        input.hash(&mut hasher);
        let out = hasher.finalize256();
        Self {
            hash: out
                .into_iter()
                .map(|x| x.to_be_bytes())
                .flatten()
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        }
    }
}

impl ToString for Hash {
    fn to_string(&self) -> String {
        base64::engine::general_purpose::URL_SAFE.encode(&self.hash)
    }
}

impl FromStr for Hash {
    type Err = DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let dat = base64::engine::general_purpose::URL_SAFE
            .decode(s)?
            .try_into()
            .map_err(|_| DecodeError::NotEnoughBytes)?;
        Ok(Self { hash: dat })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("Base64 Decode failed")]
    Base64(#[from] base64::DecodeError),
    #[error("Not enough bytes")]
    NotEnoughBytes,
}
