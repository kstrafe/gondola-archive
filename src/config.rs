pub static PLURALITY: &str = "Gondolas"; // Part of <title> of the list page
pub static LIST_TITLE: &str = "GondolaArchive"; // "name" of the /list page
pub static DEFAULT_VIDEO: &str = "/FrontPage.webm"; // FrontPage, e.g. when accessing `gondola.com`
                                                    // it will redirect to
                                                    // `gondola.com/FrontPage.webm`
                                                    // Note that you don't need to change this, you
                                                    // can just symbolically link `FrontPage.webm`
                                                    // to another video if you'd like.
pub static DESCRIPTION: &str = "Gondola webms depicting our favorite silent observer"; // Used in <head> for meta og:description
pub static SINGULAR: &str = "Gondola"; // Used in the <head> for meta og:title
pub static FORUM_NAME: &str = "evo-1"; // Disqus forum name to link the comment section to
pub static SITE_NAME: &str = "https://gondola.stravers.net"; // FQDN needed for disqus comments
pub static PORT: u16 = 50801; // TCP port to attach to
pub static NAME: &str = "gondola.stravers"; // Name of the site
pub static EMAIL: &str = "gondola@nabein.me"; // Email for contacting the site
pub static BOARD: &str = "/gs/"; // /list board title
pub static LOGLEVEL: u8 = 128; // Log level of the standard logger
pub static LOGLEVEL_IMPORTANT: u8 = 255; // Log level of the important logger (trace = 255, debug = 192,
                                         // info = 128, warn = 64, error = 0)
