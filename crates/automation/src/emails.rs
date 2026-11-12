//! `email_export`: folders of email messages (`.eml`, Outlook `.msg`) as PDFs, with each
//! attachment as its own file, every name starting with the sent date and time.

use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

#[cfg(not(target_arch = "wasm32"))]
use crate::ToolError;
#[cfg(not(target_arch = "wasm32"))]
use crate::paths::write_atomic;
use crate::{Args, Automation, Result, failed};

impl Automation {
    /// `email_export` in the web build, which has no folders to read or write.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn email_export(&mut self, _a: &Args) -> Result<Value> {
        Err(failed("email_export works in the desktop app and pdfcraft-cli, which can read and write folders"))
    }

    /// `email_export`: every `.eml` and `.msg` in `paths` (files, or folders searched with
    /// their subfolders) exported into `out_dir`, which is made if missing.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn email_export(&mut self, a: &Args) -> Result<Value> {
        let paths = a.strs("paths")?;
        if paths.is_empty() || paths.len() > 1000 {
            return Err(Args::wrong("paths", "1 to 1000 files or folders"));
        }
        let out = self.resolve(a.str("out_dir")?, true)?;
        let mut files = Vec::new();
        for p in paths {
            let src = self.resolve(p, false)?;
            if src.is_dir() {
                files.extend(pdfcraft_engine::email_files(&src));
            } else {
                files.push(src);
            }
        }
        if files.is_empty() {
            return Err(ToolError::InvalidArgs("paths hold no .eml or .msg files".into()));
        }
        std::fs::create_dir_all(&out).map_err(|e| failed(format!("{}: {e}", out.display())))?;
        let mut write = |path: &std::path::Path, bytes: &[u8]| write_atomic(path, bytes).map_err(|e| std::io::Error::other(e.to_string()));
        let report = self.session.export_email_files(&files, &out, &mut write, &mut |_, _| {});
        let pairs = |list: &[(String, String)]| list.iter().map(|(file, why)| json!({ "file": file, "why": why })).collect::<Vec<_>>();
        Ok(json!({
            "emails": report.emails,
            "written": report.written.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
            "originals": pairs(&report.originals),
            "failed": pairs(&report.failed),
        }))
    }

    /// `email_bundle` in the web build, which has no folders to read or write.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn email_bundle(&mut self, _a: &Args) -> Result<Value> {
        Err(failed("email_bundle works in the desktop app and pdfcraft-cli, which can read and write folders"))
    }

    /// `email_bundle`: the emails in `paths` as one brief bundle in `out_dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn email_bundle(&mut self, a: &Args) -> Result<Value> {
        use pdfcraft_engine::{BundleOptions, Numbering};
        let paths = a.strs("paths")?;
        if paths.is_empty() || paths.len() > 1000 {
            return Err(Args::wrong("paths", "1 to 1000 files or folders"));
        }
        let date = |key: &str| -> Result<Option<chrono::NaiveDate>> {
            match a.opt_str(key)? {
                None => Ok(None),
                Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map(Some).map_err(|_| Args::wrong(key, "a date as YYYY-MM-DD")),
            }
        };
        let (from, to) = (date("from")?, date("to")?);
        if let (Some(f), Some(t)) = (from, to)
            && f > t
        {
            return Err(Args::wrong("from", "on or before to"));
        }
        let prefix = a.opt_str("prefix")?.unwrap_or("").to_string();
        if prefix.chars().count() > 128 || prefix.contains('#') || prefix.contains(">>") {
            return Err(Args::wrong("prefix", "at most 128 characters, without # or >>"));
        }
        let start = a.0.get("start").and_then(Value::as_u64).unwrap_or(1).max(1);
        let numbering = match a.opt_str("numbering")?.unwrap_or("pages") {
            "none" => Numbering::None,
            "pages" => Numbering::Pages,
            "bates" => Numbering::Bates { prefix, start, digits: 6 },
            _ => return Err(Args::wrong("numbering", "none, pages or bates")),
        };
        let out = self.resolve(a.str("out_dir")?, true)?;
        let mut files = Vec::new();
        for p in &paths {
            let src = self.resolve(p, false)?;
            if src.is_dir() {
                files.extend(pdfcraft_engine::email_files(&src));
            } else {
                files.push(src);
            }
        }
        if files.is_empty() {
            return Err(ToolError::InvalidArgs("paths hold no .eml or .msg files".into()));
        }
        let title = match a.opt_str("title")? {
            Some(t) => t.to_string(),
            None => self.resolve(paths[0], false)?.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Brief".into()),
        };
        let index = a.opt_bool("index")?.unwrap_or(true);
        std::fs::create_dir_all(&out).map_err(|e| failed(format!("{}: {e}", out.display())))?;
        let options = BundleOptions { title, from, to, index, numbering };
        let mut write = |path: &std::path::Path, bytes: &[u8]| write_atomic(path, bytes).map_err(|e| std::io::Error::other(e.to_string()));
        let r = self.session.assemble_bundle(&files, &options, &out, &mut write, &mut |_, _| {}).map_err(|e| failed(e.to_string()))?;
        Ok(json!({
            "bundle": r.bundle.map(|p| p.to_string_lossy().into_owned()),
            "pages": r.pages,
            "emails": r.emails,
            "duplicates": r.duplicates.iter().map(|(f, n, of)| json!({ "file": f, "number": n, "duplicate_of": of })).collect::<Vec<_>>(),
            "undated": r.undated,
            "outside_range": r.outside_range,
            "not_printable": r.not_printable.iter().map(|(n, s)| json!({ "attachment": n, "saved_as": s })).collect::<Vec<_>>(),
            "unreadable": r.unreadable.iter().map(|(f, w)| json!({ "file": f, "why": w })).collect::<Vec<_>>(),
            "written": r.written.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        }))
    }
}
