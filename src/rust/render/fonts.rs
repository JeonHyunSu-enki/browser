use super::{GenericFamily, Renderer};

pub fn renderer() -> Renderer {
    let mut renderer = Renderer::new(&[]);
    renderer.fcx.collection.load_fonts_from_paths([
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/System/Library/Fonts",
        "/Library/Fonts",
    ]);
    for (generic, families) in [
        (
            GenericFamily::SansSerif,
            ["Arial", "Liberation Sans", "DejaVu Sans"],
        ),
        (
            GenericFamily::Serif,
            ["Times New Roman", "Liberation Serif", "DejaVu Serif"],
        ),
        (
            GenericFamily::Monospace,
            ["Courier New", "Liberation Mono", "DejaVu Sans Mono"],
        ),
    ] {
        let ids: Vec<_> = families
            .iter()
            .filter_map(|name| renderer.fcx.collection.family_id(name))
            .collect();
        if !ids.is_empty() {
            renderer
                .fcx
                .collection
                .set_generic_families(generic, ids.into_iter());
        }
    }
    renderer
}

pub fn family(value: &str) -> String {
    value
        .split(',')
        .map(|part| {
            match part
                .trim()
                .trim_matches(['\'', '"'])
                .to_ascii_lowercase()
                .as_str()
            {
                "arial" => "Arial, Liberation Sans".to_owned(),
                "times new roman" => "Times New Roman, Liberation Serif".to_owned(),
                "courier new" => "Courier New, Liberation Mono".to_owned(),
                _ => part.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
