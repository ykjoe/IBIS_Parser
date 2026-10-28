use toml::Table;
use std::path::Path;
use std::fs;
use std::error::Error;

pub struct IbstomlMap {
    toml_map: Table,
}

enum IbstomlSchemaFormat {
    FileHeader,
    Text,
    Textlines,
    Table,
    IVTable,
    VTTable,
}

enum IbstomlParamFormat {
    None,
    Text,
    Quantity,
    Corner
}

impl IbstomlMap {
    // ============================================================
    // # Loaders
    // Read the ibstoml file and output it as toml map
    // ============================================================
    
    /// Load ibstoml file from local path
    /// 
    /// # Arguments
    /// * `path` - path to `.ibs.toml` file
    /// 
    /// # Errors
    /// Return error when file read failed or TOML parsing failed
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let content = fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Load ibstoml from string slice
    /// 
    /// # Errors
    /// Return error when TOML parsing failed
    pub fn from_str(content: &str) -> Result<Self, Box<dyn Error>> {
        let toml_map: Table = toml::from_str(content)?;
        Ok(Self { toml_map })
    }

    // ============================================================
    // # extrators
    // 
    // ============================================================

    fn get_by_keyword_path<'a>(&'a self, kw_path: &str) -> Option<&'a toml::Value> {
        // split path & try to find
        let exact_query = |path: &str| -> Option<&'a toml::Value> {
            let mut parts = path.split('.');
            let first_part = parts.next()?;
            let mut current_val = self.toml_map.get(first_part)?;

            for part in parts {
                current_val = match current_val {
                    toml::Value::Table(tbl) => tbl.get(part)?,
                    toml::Value::Array(arr) => {
                        let idx = part.parse::<usize>().unwrap_or(0);
                        arr.get(idx)?
                    }
                    _ => return None,
                };
            }
            Some(current_val)
        };

        // Match path
        if let Some(val) = exact_query(kw_path) {
            if let toml::Value::Table(tbl) = val {
                if let Some(last_segment) = kw_path.split('.').last() {
                    let lower_last = last_segment.to_lowercase();
                    if let Some(sub) = tbl.get(&lower_last) {
                        return Some(sub);
                    }
                    if let Some(sub) = tbl.get("__value__") {
                        return Some(sub);
                    }
                }
            }
            return Some(val);
        }

        // Match lowercase last & __value__ last
        let parts: Vec<&str> = kw_path.split('.').collect();
        if let Some(last_segment) = parts.last() {
            let lower_last = last_segment.to_lowercase();

            let path_with_lower = format!("{}.{}", kw_path, lower_last);
            if let Some(val) = exact_query(&path_with_lower) {
                return Some(val);
            }

            let path_with_value = format!("{}.__value__", kw_path);
            if let Some(val) = exact_query(&path_with_value) {
                return Some(val);
            }
        }

        None
    }

    fn get_by_param_path<'a>(&'a self, param_path: &str) -> Option<&'a toml::Value> {
        let mut parts = param_path.split('.');
        let first_part = parts.next()?;
        
        let mut current_val = self.toml_map.get(first_part)?;

        for part in parts {
            current_val = match current_val {
                toml::Value::Table(tbl) => tbl.get(part)?,
                toml::Value::Array(arr) => {
                    let idx = part.parse::<usize>().ok()?;
                    arr.get(idx)?
                }
                _ => return None,
            };
        }
        Some(current_val)
    }

    pub fn extract_text(&self, keyword_path: &str) -> Result<String, Box<dyn Error>> {
        let value = self.get_by_keyword_path(keyword_path)
            .ok_or_else(|| format!("Keyword path not found: {}", keyword_path))?;
        let text = value.as_str()
            .ok_or_else(|| format!("Path '{}' is not found", keyword_path))?;

        Ok(text.to_string())
    }

    pub fn extract_textlines(&self, keyword_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let value = self.get_by_keyword_path(keyword_path)
            .ok_or_else(|| format!("Keyword path not found: {}", keyword_path))?;
        
        let text_arr = value.as_array()
            .ok_or_else(|| format!("Path '{}' is not an array", keyword_path))?;

        let mut lines = Vec::with_capacity(text_arr.len());
        for item in text_arr {
            let s = item.as_str()
                .ok_or_else(|| "Text lines error!")?;
            lines.push(s.to_string());
        }

        Ok(lines)
    }

    pub fn extract_table(&self, keyword_path: &str) {
        
    }

    pub fn extract_iv_table(&self, keyword_path: &str) {

    }

    pub fn extract_vt_table(&self, keyword_path: &str) {
        
    }
}
