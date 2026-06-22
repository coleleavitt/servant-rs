/// Where the client points. Default ports: 80 (http) / 443 (https).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    /// `http` or `https`.
    pub scheme: Scheme,
    /// Host name.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Base path prefix (without trailing slash), prepended to request paths.
    pub path: String,
}

/// URL scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// `http://`
    Http,
    /// `https://`
    Https,
}

impl BaseUrl {
    /// `http://host:port` with no base path.
    pub fn http(host: impl Into<String>, port: u16) -> Self {
        BaseUrl {
            scheme: Scheme::Http,
            host: host.into(),
            port,
            path: String::new(),
        }
    }

    /// The scheme as a string.
    pub fn scheme_str(&self) -> &'static str {
        match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    /// Build the absolute URL for a request target (which begins with `/`).
    pub fn url_for(&self, target: &str) -> String {
        format!(
            "{}://{}:{}{}{}",
            self.scheme_str(),
            self.host,
            self.port,
            self.path,
            target
        )
    }
}
