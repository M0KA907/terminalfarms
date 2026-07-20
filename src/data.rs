use std::collections::HashSet;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct CropCatalog {
    pub crops: Vec<CropDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CropDefinition {
    pub id: String,
    pub name: String,
    pub seed_price: u64,
    pub sell_price: u64,
    pub grow_seconds: u64,
    pub unlock_earnings: u64,
    pub color: String,
    pub art: [[String; 2]; 6],
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpgradeCatalog {
    pub upgrades: Vec<UpgradeDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpgradeDefinition {
    pub id: String,
    pub name: String,
    pub kind: UpgradeKind,
    pub base_price: u64,
    pub unlock_earnings: u64,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeKind {
    AutoTill,
    AutoSow,
    GrowthBoost,
    AutoHarvest,
    AutoSell,
}

#[derive(Debug, Error)]
pub enum DataError {
    #[error("embedded crop data is invalid: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("crop catalog must contain at least one crop")]
    Empty,
    #[error("crop id `{0}` is duplicated")]
    DuplicateId(String),
    #[error("crop `{0}` has invalid prices, timing, or stage art")]
    InvalidCrop(String),
    #[error("upgrade catalog must contain at least one upgrade")]
    EmptyUpgrades,
    #[error("upgrade id `{0}` is duplicated")]
    DuplicateUpgradeId(String),
    #[error("upgrade `{0}` has invalid pricing, timing, or levels")]
    InvalidUpgrade(String),
}

impl CropCatalog {
    pub fn embedded() -> Result<Self, DataError> {
        let catalog: Self = toml::from_str(include_str!("../assets/crops.toml"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&CropDefinition> {
        self.crops.iter().find(|crop| crop.id == id)
    }

    fn validate(&self) -> Result<(), DataError> {
        if self.crops.is_empty() {
            return Err(DataError::Empty);
        }

        let mut ids = HashSet::new();
        for crop in &self.crops {
            if !ids.insert(crop.id.as_str()) {
                return Err(DataError::DuplicateId(crop.id.clone()));
            }
            if crop.id.is_empty()
                || crop.name.is_empty()
                || crop.seed_price == 0
                || crop.sell_price == 0
                || crop.grow_seconds == 0
                || crop
                    .art
                    .iter()
                    .flatten()
                    .any(|line| line.chars().count() != 3)
            {
                return Err(DataError::InvalidCrop(crop.id.clone()));
            }
        }
        Ok(())
    }
}

impl UpgradeCatalog {
    pub fn embedded() -> Result<Self, DataError> {
        let catalog: Self = toml::from_str(include_str!("../assets/upgrades.toml"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&UpgradeDefinition> {
        self.upgrades.iter().find(|upgrade| upgrade.id == id)
    }

    fn validate(&self) -> Result<(), DataError> {
        if self.upgrades.is_empty() {
            return Err(DataError::EmptyUpgrades);
        }
        let mut ids = HashSet::new();
        for upgrade in &self.upgrades {
            if !ids.insert(upgrade.id.as_str()) {
                return Err(DataError::DuplicateUpgradeId(upgrade.id.clone()));
            }
            let timed = upgrade.kind != UpgradeKind::GrowthBoost;
            if upgrade.id.is_empty()
                || upgrade.name.is_empty()
                || upgrade.base_price == 0
                || (timed && upgrade.interval_seconds == 0)
                || (!timed && upgrade.interval_seconds != 0)
            {
                return Err(DataError::InvalidUpgrade(upgrade.id.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid() {
        let catalog = CropCatalog::embedded().unwrap();
        assert_eq!(catalog.crops.first().unwrap().id, "radish");
        assert!(catalog.crops.len() >= 50);
    }

    #[test]
    fn catalog_progression_is_ordered_and_profitable() {
        let catalog = CropCatalog::embedded().unwrap();
        let mut last_unlock = 0;
        let mut last_grow = 0;
        for crop in &catalog.crops {
            assert!(
                crop.unlock_earnings >= last_unlock,
                "crop `{}` breaks unlock ordering",
                crop.id
            );
            last_unlock = crop.unlock_earnings;
            assert!(
                crop.grow_seconds >= last_grow && crop.grow_seconds <= 9_000,
                "crop `{}` breaks growth-time pacing",
                crop.id
            );
            last_grow = crop.grow_seconds;
            assert!(
                crop.sell_price >= crop.seed_price.saturating_mul(3),
                "crop `{}` seed is too expensive for its sell price",
                crop.id
            );
        }
    }

    #[test]
    fn catalog_colors_are_known() {
        let known = ["red", "yellow", "magenta", "green", "blue", "white"];
        let catalog = CropCatalog::embedded().unwrap();
        for crop in &catalog.crops {
            assert!(
                known.contains(&crop.color.as_str()),
                "crop `{}` uses unknown color `{}`",
                crop.id,
                crop.color
            );
        }
    }

    #[test]
    fn embedded_upgrades_are_valid() {
        let catalog = UpgradeCatalog::embedded().unwrap();
        assert_eq!(catalog.upgrades.first().unwrap().id, "cultivator");
        assert_eq!(catalog.upgrades.len(), 5);
    }
}
