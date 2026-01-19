use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Presentation {
    pub title: String,
    pub author: Option<String>,
    pub theme: Option<PresentationTheme>,
    pub slides: Vec<Slide>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresentationTheme {
    #[default]
    Light,
    Dark,
    Corporate,
    Minimal,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlideLayout {
    Title,
    Content,
    Section,
    Blank,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Slide {
    pub id: String,
    pub layout: SlideLayout,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub elements: Vec<SlideElement>,
    pub notes: Option<String>, // Speaker notes
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementType {
    Text,
    Image,
    BulletList,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SlideElement {
    #[serde(rename = "type")]
    pub element_type: ElementType,
    pub content: ElementContent,
    pub position: Option<ElementPosition>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ElementContent {
    Text(String),
    Bullets(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ElementPosition {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
