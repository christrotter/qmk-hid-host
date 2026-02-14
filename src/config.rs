use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

fn default_api_endpoint() -> String {
    "http://10.0.0.1/json/state".to_string()
}

fn default_chrome_tab_mappings() -> Option<HashMap<String, String>> {
    let mut mappings = HashMap::new();
    mappings.insert("cad.onshape.com".to_string(), "Fusion".to_string());
    Some(mappings)
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub devices: Vec<Device>,
    pub layouts: Vec<String>,
    #[serde(default = "default_api_endpoint")]
    pub api_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconnect_delay: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default = "default_chrome_tab_mappings")]
    pub chrome_tab_mappings: Option<HashMap<String, String>>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(serialize_with = "hex_to_string", deserialize_with = "string_to_hex")]
    pub product_id: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_page: Option<u16>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get_config() -> &'static Config {
    CONFIG.get().unwrap()
}

pub fn load_config(path: PathBuf) -> &'static Config {
    if let Some(config) = CONFIG.get() {
        return config;
    }

    let default_config = Config {
        devices: vec![Device {
            name: None,
            product_id: 0x0844,
            usage: None,
            usage_page: None,
        }],
        layouts: vec!["en".to_string()],
        api_endpoint: default_api_endpoint(),
        reconnect_delay: None,
        chrome_tab_mappings: default_chrome_tab_mappings(),
    };

    if let Ok(file) = std::fs::read_to_string(&path) {
        let config = serde_json::from_str::<Config>(&file)
            .map_err(|e| tracing::error!("Incorrect config file: {}", e))
            .unwrap();

        // Check if we need to update the config file (e.g., if api_endpoint or chromeTabMappings was missing)
        let needs_update = !file.contains("apiEndpoint") || !file.contains("chromeTabMappings");

        if needs_update {
            let file_content = serde_json::to_string_pretty(&config).unwrap();
            std::fs::write(&path, &file_content)
                .map_err(|e| tracing::error!("Error while updating config file at {:?}: {}", path, e))
                .unwrap();
            tracing::info!("Config file updated at {:?} with new fields", path);
        }

        // Validate that api_endpoint is not empty
        if config.api_endpoint.is_empty() {
            panic!("ERROR: api_endpoint is not configured in {:?}. Please set a valid API endpoint in the config file.", path);
        }

        return CONFIG.get_or_init(|| config);
    }

    let file_content = serde_json::to_string_pretty(&default_config).unwrap();
    std::fs::write(&path, &file_content)
        .map_err(|e| tracing::error!("Error while saving config file to {:?}: {}", path, e))
        .unwrap();
    tracing::info!("New config file created at {:?}", path);

    CONFIG.get_or_init(|| default_config)
}

fn string_to_hex<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: &str = serde::Deserialize::deserialize(deserializer)?;
    let hex = value.trim_start_matches("0x");
    return u16::from_str_radix(hex, 16).map_err(serde::de::Error::custom);
}

fn hex_to_string<S>(value: &u16, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("0x{:04x}", value))
}
