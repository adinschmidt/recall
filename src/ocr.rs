use anyhow::{Context, Result};
use image::DynamicImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use std::sync::LazyLock;

// Embed models directly into the binary at compile time
static DETECTION_MODEL_DATA: &[u8] = include_bytes!("../models/text-detection.rten");
static RECOGNITION_MODEL_DATA: &[u8] = include_bytes!("../models/text-recognition.rten");

// Lazily initialize the OCR engine
static OCR_ENGINE: LazyLock<OcrEngine> = LazyLock::new(|| {
    let detection_model =
        Model::load(DETECTION_MODEL_DATA.to_vec()).expect("Failed to load text detection model");
    let recognition_model =
        Model::load(RECOGNITION_MODEL_DATA.to_vec()).expect("Failed to load text recognition model");

    OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .expect("Failed to initialize OCR engine")
});

/// Perform OCR on an image and return the extracted text.
pub fn extract_text(image: &DynamicImage) -> Result<String> {
    // Convert DynamicImage to RGB8 format expected by ocrs
    let rgb_image = image.to_rgb8();
    let img_source = ImageSource::from_bytes(rgb_image.as_raw(), rgb_image.dimensions())
        .context("Failed to create image source")?;

    let ocr_input = OCR_ENGINE
        .prepare_input(img_source)
        .context("Failed to prepare OCR input")?;

    let word_rects = OCR_ENGINE
        .detect_words(&ocr_input)
        .context("Failed to detect words")?;

    let line_rects = OCR_ENGINE.find_text_lines(&ocr_input, &word_rects);

    let line_texts = OCR_ENGINE
        .recognize_text(&ocr_input, &line_rects)
        .context("Failed to recognize text")?;

    let text: String = line_texts
        .iter()
        .filter_map(|line| line.as_ref().map(|l| l.to_string()))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(text)
}
