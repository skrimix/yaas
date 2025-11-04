use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use anyhow::{Context, Result, anyhow};
use resand::{
    res_value::ResValueType,
    string_pool::StringPoolHandler,
    table::{ResTable, ResTableEntryValue},
    xmltree::{ResXMLTreeAttribute, XMLTree},
};
use tracing::instrument;
use zip::ZipArchive;

/// Basic APK metadata parsed from `AndroidManifest.xml` (and optionally `resources.arsc`).
#[derive(Debug, Clone)]
pub struct ApkInfo {
    pub application_label: Option<String>,
    pub package_name: String,
    pub version_code: Option<u32>,
    pub version_name: Option<String>,
}

/// Parse minimal info from an APK using `resand` only (no axmldecoder/aapt).
/// Best-effort: returns `Ok` with whatever could be extracted. Fails for
/// unrecoverable issues like missing/invalid `AndroidManifest.xml`.
#[instrument(ret, level = "debug", fields(apk_path = %apk_path.as_ref().display()))]
pub fn get_apk_info(apk_path: impl AsRef<Path>) -> Result<ApkInfo> {
    let apk_path = apk_path.as_ref();
    if !apk_path.exists() {
        return Err(anyhow!("APK file not found: {}", apk_path.display()));
    }

    // 1) Open the APK zip and read the two files we care about.
    let file =
        File::open(apk_path).with_context(|| format!("Failed to open {}", apk_path.display()))?;
    let mut zip = ZipArchive::new(file).with_context(|| "Failed to read APK as zip")?;

    let manifest_bytes = read_zip_file(&mut zip, "AndroidManifest.xml")
        .context("Failed to read AndroidManifest.xml")?;

    let resources_bytes = read_zip_file(&mut zip, "resources.arsc").ok();

    let xml = XMLTree::read(&mut Cursor::new(&manifest_bytes))
        .map_err(|e| anyhow!("Failed to parse binary AndroidManifest.xml: {e}"))?;

    let mut app_label: Option<String> = None;
    let mut pkg_name: Option<String> = None;
    let mut vcode: Option<u32> = None;
    let mut vname: Option<String> = None;

    // Package name and versions from <manifest>
    if let Some(manifest_el) =
        xml.root.get_elements(&["manifest"], &xml.string_pool).into_iter().next()
    {
        if let Some(attr) = manifest_el.get_attribute("package", &xml.string_pool)
            && let Some(s) = read_attr_string(attr, &xml.string_pool)
        {
            pkg_name = Some(s);
        }
        if let Some(attr) = manifest_el.get_attribute("versionCode", &xml.string_pool) {
            vcode = read_attr_u32(attr).or_else(|| {
                read_attr_string(attr, &xml.string_pool).and_then(|s| s.parse::<u32>().ok())
            });
        }
        if let Some(attr) = manifest_el.get_attribute("versionName", &xml.string_pool) {
            vname = read_attr_string(attr, &xml.string_pool);
        }
    }

    // Application label from <application android:label>
    if let Some(application) =
        xml.root.get_elements(&["manifest", "application"], &xml.string_pool).into_iter().next()
        && let Some(attr) = application.get_attribute("label", &xml.string_pool)
    {
        // If it is literal string, read it. If it is reference, use resand table if present.
        if let Some(txt) = read_attr_string(attr, &xml.string_pool) {
            app_label = literal_or_none(txt);
        } else if let Some(res_bytes) = resources_bytes.as_ref()
            && let Some(txt) = resolve_label_resand(attr, res_bytes)
        {
            app_label = Some(txt);
        }
    }

    let package_name = pkg_name.ok_or_else(|| anyhow!("Manifest missing 'package' attribute"))?;
    let version_code = vcode;
    let version_name = vname;
    let application_label = app_label;

    Ok(ApkInfo { application_label, package_name, version_code, version_name })
}

// --- helpers ---

fn read_zip_file(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut file = zip.by_name(name)?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// If the label looks like a literal (e.g. "Hello"), return `Some(literal)`.
/// If it's an `@resource` or `0x...` id, return `None`.
fn literal_or_none(s: String) -> Option<String> {
    let t = s.trim();
    if t.starts_with('@') || t.starts_with("0x") || t.starts_with("ResourceValueType::Reference/") {
        None
    } else {
        Some(s)
    }
}

// --- resand helpers ---

/// Read a string attribute value from an XML attribute using the given pool.
fn read_attr_string(attr: &ResXMLTreeAttribute, pool: &StringPoolHandler) -> Option<String> {
    match attr.typed_value.data {
        ResValueType::String(sref) => pool.resolve(sref).map(|s| s.to_string()),
        _ => None,
    }
}

/// Read an integer attribute value (decimal or hex) as `u32`.
fn read_attr_u32(attr: &ResXMLTreeAttribute) -> Option<u32> {
    match attr.typed_value.data {
        ResValueType::IntDec(v) | ResValueType::IntHex(v) => Some(v),
        _ => None,
    }
}

fn resolve_label_resand(attr: &ResXMLTreeAttribute, resources_bytes: &[u8]) -> Option<String> {
    // Only attempt reference resolution when attribute is a reference
    let ref_id = match attr.typed_value.data {
        ResValueType::Reference(r) | ResValueType::DynamicReference(r) => Some(r),
        _ => None,
    }?;

    let table = match ResTable::read_all(&mut Cursor::new(resources_bytes)) {
        Ok(t) => t,
        Err(_) => return None,
    };

    // Try all packages until resolve succeeds
    let mut i = 0usize;
    while let Some(pkg) = table.packages.get(i) {
        if let Some(entry) = pkg.resolve_ref(ref_id)
            && let ResTableEntryValue::ResValue(v) = &entry.data
            && let ResValueType::String(sref) = v.data.data
            && let Some(s) = table.string_pool.resolve(sref)
        {
            return Some(s.to_string());
        }
        i += 1;
    }
    None
}
