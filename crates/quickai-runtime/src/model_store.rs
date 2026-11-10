//! Where an engine that keeps its models in a store of its own keeps a model's weights (C213): Ollama's store has a
//! manifest for each name (`manifests/<host>/<namespace>/<model>/<tag>`, JSON) whose layer of the model's media type
//! names a blob (`blobs/sha256-<hex>`), which is a GGUF file. With it, what the file says of the model (its layers, the
//! bytes a token of context takes, how long a context it was trained for) is read as it is for a file of llama.cpp's,
//! and the panel can say where a model of that engine fits.

use std::path::{Path, PathBuf};

/// The media type of the layer that is the weights.
const MODEL_LAYER: &str = "application/vnd.ollama.image.model";

/// The registry a short name is of, and the namespace of a name with none.
const DEFAULT_HOST: &str = "registry.ollama.ai";
const DEFAULT_NAMESPACE: &str = "library";

/// Where the manifest of `name` is in `store`: `qwen3:1.7b` is `registry.ollama.ai/library/qwen3/1.7b`, `me/model` is
/// `registry.ollama.ai/me/model/latest`, `hf.co/org/repo:Q4_K_M` is `hf.co/org/repo/Q4_K_M`. A name with a part that
/// could leave the folder is none.
pub fn manifest_path(store: &Path, name: &str) -> Option<PathBuf> {
    let (repo, tag) = match name.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo, tag),
        _ => (name, "latest"),
    };
    let parts: Vec<&str> = repo.split('/').collect();
    let safe = |p: &str| !p.is_empty() && p != "." && p != ".." && !p.contains('\\');
    if !parts.iter().all(|p| safe(p)) || !safe(tag) {
        return None;
    }
    let mut path = store.join("manifests");
    match parts.as_slice() {
        [model] => path.extend([DEFAULT_HOST, DEFAULT_NAMESPACE, model]),
        [namespace, model] => path.extend([DEFAULT_HOST, namespace, model]),
        many => path.extend(many.iter()),
    }
    path.push(tag);
    Some(path)
}

/// The weights of `name` in `store`, when its manifest is there and names a blob that is.
pub fn weights_in_store(store: &Path, name: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(manifest_path(store, name)?).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    let digest = manifest
        .get("layers")?
        .as_array()?
        .iter()
        .find(|layer| layer.get("mediaType").and_then(|m| m.as_str()) == Some(MODEL_LAYER))?
        .get("digest")?
        .as_str()?;
    let (algorithm, hex) = digest.split_once(':')?;
    if !algorithm.chars().all(|c| c.is_ascii_alphanumeric())
        || !hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let blob = store.join("blobs").join(format!("{algorithm}-{hex}"));
    blob.is_file().then_some(blob)
}

/// The stores to look in, first the one the engine is told to use: Ollama's `models_dir`, then `OLLAMA_MODELS`, then
/// its own folder in the home and the one of the system's service.
pub fn ollama_stores(told: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut stores: Vec<PathBuf> = told.map(Path::to_path_buf).into_iter().collect();
    if let Some(env) = std::env::var_os("OLLAMA_MODELS").filter(|v| !v.is_empty()) {
        stores.push(PathBuf::from(env));
    }
    if let Some(home) = home {
        stores.push(home.join(".ollama").join("models"));
    }
    stores.push(PathBuf::from("/usr/share/ollama/.ollama/models"));
    stores.dedup();
    stores
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::scratch;

    #[test]
    fn a_name_is_its_manifest_in_the_store() {
        let store = Path::new("/s");
        assert_eq!(
            manifest_path(store, "qwen3:1.7b").unwrap(),
            Path::new("/s/manifests/registry.ollama.ai/library/qwen3/1.7b")
        );
        assert_eq!(
            manifest_path(store, "me/model").unwrap(),
            Path::new("/s/manifests/registry.ollama.ai/me/model/latest")
        );
        assert_eq!(
            manifest_path(store, "hf.co/org/repo:Q4_K_M").unwrap(),
            Path::new("/s/manifests/hf.co/org/repo/Q4_K_M")
        );
        assert_eq!(manifest_path(store, "../x:y"), None);
        assert_eq!(manifest_path(store, "a/../../b"), None);
    }

    #[test]
    fn the_weights_are_the_blob_of_the_model_layer() {
        let dir = scratch("model-store");
        let store: &Path = dir.as_ref();
        let manifest = manifest_path(store, "qwen3:1.7b").unwrap();
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::create_dir_all(store.join("blobs")).unwrap();
        std::fs::write(
            &manifest,
            r#"{"schemaVersion":2,"layers":[
                {"mediaType":"application/vnd.ollama.image.template","digest":"sha256:aa","size":1},
                {"mediaType":"application/vnd.ollama.image.model","digest":"sha256:bb12","size":5}]}"#,
        )
        .unwrap();
        assert_eq!(
            weights_in_store(store, "qwen3:1.7b"),
            None,
            "the blob is not there yet"
        );
        std::fs::write(store.join("blobs/sha256-bb12"), b"GGUF").unwrap();
        assert_eq!(
            weights_in_store(store, "qwen3:1.7b"),
            Some(store.join("blobs/sha256-bb12"))
        );
        assert_eq!(weights_in_store(store, "qwen3:4b"), None);
    }
}
