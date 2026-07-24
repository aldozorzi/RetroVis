fn main() {
    // Embed the app icon from the resource file
    let _ = embed_resource::compile("icon.rc", embed_resource::NONE);
}
