fn main() {
    println!("cargo:rerun-if-changed=settings.rc");
    embed_resource::compile("settings.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
