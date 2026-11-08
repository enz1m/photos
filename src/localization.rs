use enzui_iced_core::localization::{CatalogBuilder, Localizer, preferred_locales};

pub(crate) fn catalog() -> Localizer {
    let mut catalog = CatalogBuilder::new();
    for (locale, resource) in [
        ("en-US", include_str!("../assets/locales/en-US.ftl")),
        ("de", include_str!("../assets/locales/de.ftl")),
        ("ar", include_str!("../assets/locales/ar.ftl")),
    ] {
        catalog
            .add_resource(locale, resource)
            .expect("valid Photos catalog");
    }
    Localizer::new(
        catalog.build(),
        preferred_locales(),
        "en-US".parse().expect("valid locale"),
    )
}
