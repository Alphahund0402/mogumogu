fn main() {
    #[cfg(feature = "desktop")]
    {
        println!("cargo:rerun-if-changed=ui");
        println!("cargo:rerun-if-changed=assets");
        // Images (PNG/JPEG/SVG) are embedded as files and decoded at runtime.
        // Pre-rendering for the software renderer would also rasterise the
        // font into a single weight, so text uses the system fonts instead.
        let config = slint_build::CompilerConfiguration::new()
            .with_style("fluent-light".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
        slint_build::compile_with_config("ui/app.slint", config)
            .expect("Slint UI could not be compiled; inspect the diagnostic above.");
    }
}
