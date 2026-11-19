fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/openstart.ico");
        res.set("FileDescription", "OpenStart - Start menu for Windows");
        res.set("ProductName", "OpenStart");
        res.set("LegalCopyright", "MIT");
        let _ = res.compile();
    }
}
