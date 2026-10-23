//! Loads remote images off the UI thread and hands them to Slint models.

use cr_core::images::ImageCache;
use slint::{Image, Model, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

thread_local! {
    static MEMORY: RefCell<HashMap<(String, u32), Image>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct Images {
    cache: Arc<ImageCache>,
    rt: tokio::runtime::Handle,
}

impl Images {
    pub fn new(cache: ImageCache, rt: tokio::runtime::Handle) -> Self {
        Self { cache: Arc::new(cache), rt }
    }

    /// Calls `done` on the UI thread with the decoded image.
    pub fn load(&self, url: String, max_width: u32, done: impl FnOnce(Image) + 'static) {
        if url.is_empty() {
            return;
        }
        let key = (url.clone(), max_width);
        if let Some(img) = MEMORY.with(|m| m.borrow().get(&key).cloned()) {
            done(img);
            return;
        }
        let cache = self.cache.clone();
        let task = self.rt.spawn(async move { cache.load(&url, max_width).await });
        let _ = slint::spawn_local(async move {
            match task.await {
                Ok(Ok(d)) => {
                    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&d.rgba, d.width, d.height);
                    let img = Image::from_rgba8(buf);
                    MEMORY.with(|m| {
                        let mut m = m.borrow_mut();
                        if m.len() > 600 {
                            m.clear();
                        }
                        m.insert(key, img.clone());
                    });
                    done(img);
                }
                Ok(Err(e)) => tracing::debug!("imagem falhou: {e:#}"),
                Err(e) => tracing::debug!("tarefa de imagem falhou: {e}"),
            }
        });
    }

    /// Loads one image per row of `model` and writes it back, if the row still
    /// holds the same item (`id_of`) when the image arrives.
    pub fn fill<T: Clone + 'static>(
        &self,
        model: &Rc<VecModel<T>>,
        urls: Vec<Option<String>>,
        max_width: u32,
        id_of: fn(&T) -> String,
        set: fn(&mut T, Image),
    ) {
        for (i, url) in urls.into_iter().enumerate() {
            let Some(url) = url else { continue };
            let Some(row) = model.row_data(i) else { continue };
            let id = id_of(&row);
            let weak = Rc::downgrade(model);
            self.load(url, max_width, move |img| {
                let Some(model) = weak.upgrade() else { return };
                if let Some(mut row) = model.row_data(i) {
                    if id_of(&row) == id {
                        set(&mut row, img);
                        model.set_row_data(i, row);
                    }
                }
            });
        }
    }
}
