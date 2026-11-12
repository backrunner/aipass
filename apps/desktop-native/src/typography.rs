//! Match the reference's `-webkit-font-smoothing: antialiased` without changing
//! the user's persisted defaults. GPUI reads this process-local Foundation
//! setting before caching its glyph dilation policy.
pub fn init() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{
            ns_string, NSArgumentDomain, NSMutableCopying, NSNumber, NSUserDefaults,
        };
        let defaults = NSUserDefaults::standardUserDefaults();
        // SAFETY: Foundation supplies this NSString constant. The existing
        // domain is copied so unrelated launch arguments remain intact; values
        // are valid Foundation property-list objects. Volatile domains are
        // discarded at process exit and never synchronized to persistent state.
        unsafe {
            let domain = defaults
                .volatileDomainForName(NSArgumentDomain)
                .mutableCopy();
            domain.insert(
                ns_string!("AppleFontSmoothing"),
                &NSNumber::numberWithInt(0),
            );
            defaults.setVolatileDomain_forName(&domain, NSArgumentDomain);
        }
    }
}
