fn main() {
    println!("cargo:rerun-if-changed=settings.rc");
    println!("cargo:rerun-if-changed=settings.ico");
    embed_resource::compile("settings.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
