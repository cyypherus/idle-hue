use std::sync::Arc;
use tokio::sync::oneshot;

pub(crate) fn sample_color() -> oneshot::Receiver<Option<[f32; 3]>> {
    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

    #[cfg(target_os = "macos")]
    {
        use block2::RcBlock;
        use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};

        let sampler = NSColorSampler::new();

        let handler = RcBlock::new(move |color: *mut NSColor| {
            let result = if color.is_null() {
                None
            } else {
                let color = unsafe { &*color };
                let srgb_space = NSColorSpace::sRGBColorSpace();
                let srgb_color = color.colorUsingColorSpace(&srgb_space);
                srgb_color.map(|c|
                    [
                        c.redComponent() as f32,
                        c.greenComponent() as f32,
                        c.blueComponent() as f32,
                    ]
                )
            };
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(result);
            }
        });

        unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(None);
        }
    }

    rx
}
