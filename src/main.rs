#!(feature(proc_macro_hygiene)]
use {
    self::config::*,
    actix_files::NamedFile,
    actix_service::Service,
    actix_web::{
        cookie::Cookie,
        error,
        http::{header::ContentType, StatusCode},
        web,
        web::Data,
        App, HttpRequest, HttpResponse, HttpServer, Responder,
    },
    chrono::{prelude::*, DateTime},
    derive_more::Display,
    fast_logger::{error, info, trace, warn, Generic, InDebug, Logger},
    indexmap::IndexMap,
    maud::{html, Markup, PreEscaped, DOCTYPE},
    rand::Rng,
    rand_pcg::Pcg64Mcg as Random,
    serde_derive::Deserialize,
    sha2::{Digest, Sha512},
    std::{
        cell::RefCell,
        cmp,
        fs::{read_dir, File},
        io::{self, Read, Write},
        num::ParseIntError,
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread,
        time::{Duration, Instant, SystemTime},
    },
};

// ---

mod config;
mod writer;

// ---

macro_rules! benchmark {
    ($lgr:expr, $e:expr) => {{
        let before = Instant::now();
        let result = $e;
        let after = Instant::now();
        $lgr(after - before);
        result
    }};
}

// ---

static COOKIE_NAME: &str = "autoplay";
static COOKIE_AUTOPLAY_RANDOM_VALUE: &str = "random";
static COOKIE_AUTOPLAY_NEXT_VALUE: &str = "next";

fn header(style_count: u64) -> Markup {
    let december = Utc::now().month() == 12;
    html! {
        meta charset="UTF-8";
        meta name="viewport" content="width=device-width,maximum-scale=1,minimum-scale=1,minimal-ui";
        @if december {
            link rel="icon" type="image/png" href="/files/favicon/16_christmas.png";
            link rel="icon" type="image/png" href="/files/favicon/32_christmas.png";
            link rel="icon" type="image/png" href="/files/favicon/64_christmas.png";
            link rel="icon" type="image/png" href="/files/favicon/128_christmas.png";
        } @else {
            link rel="icon" type="image/png" href="/files/favicon/16.png";
            link rel="icon" type="image/png" href="/files/favicon/32.png";
            link rel="icon" type="image/png" href="/files/favicon/64.png";
            link rel="icon" type="image/png" href="/files/favicon/128.png";
        }
        link rel="stylesheet" type="text/css" href="/files/css/reset.css";
        link rel="stylesheet" type="text/css" href=(&("/files/css/style.css?x=".to_string() + &style_count.to_string()));
        meta name="description" content=(DESCRIPTION);
        meta property="og:title" content=(SINGULAR);
        meta property="og:description" content=(DESCRIPTION);
        @if december {
            meta property="og:image" content="/files/favicon/128.png";
        } @else {
            meta property="og:image" content="/files/favicon/128_christmas.png";
        }
    }
}

fn header_list() -> Markup {
    html! {
        meta charset="UTF-8";
        meta name="viewport" content="width=device-width,maximum-scale=1,minimum-scale=1,minimal-ui";
        link rel="icon" type="image/png" href="/files/favicon/16.png";
        link rel="icon" type="image/png" href="/files/favicon/32.png";
        link rel="icon" type="image/png" href="/files/favicon/64.png";
        link rel="icon" type="image/png" href="/files/favicon/128.png";
        link rel="stylesheet" id="pageStyle" href="/files/css/yotsuba.css" title="switch";
        link rel="canonical" href=(format!("{}/list", SITE_NAME));
        meta name="description" content=(DESCRIPTION);
        meta property="og:title" content=(SINGULAR);
        meta property="og:description" content=(DESCRIPTION);
        meta property="og:image" content="/files/128.png";
    }
}

// ---

async fn index() -> impl Responder {
    HttpResponse::PermanentRedirect()
        .insert_header(("Location", DEFAULT_VIDEO))
        .finish()
}

#[derive(Debug, Display)]
enum MyError {
    #[display(fmt = "unauthorized")]
    Unauthorized,
}

impl std::error::Error for MyError {}

impl error::ResponseError for MyError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        match self {
            MyError::Unauthorized => StatusCode::UNAUTHORIZED,
        }
    }
}

async fn get_file(state: web::Data<State>, req: HttpRequest) -> actix_web::Result<NamedFile> {
    let mut path = PathBuf::from("files/");
    let rest = req
        .match_info()
        .query("filename")
        .parse::<PathBuf>()
        .unwrap();
    path.push(&rest);

    for item in path.components() {
        if matches!(item, std::path::Component::ParentDir) {
            return Err(MyError::Unauthorized.into());
        }
    }

    match NamedFile::open(path) {
        Ok(file) => Ok(file),
        Err(err) => {
            warn!(state.lgr_important.borrow(), "Request for non-existent file"; "filename" => InDebug(&rest));
            Err(err.into())
        }
    }
}

fn increment_view_count(state: &web::Data<State>, info: &str) {
    let mut video_infos = state.video_info.write().unwrap();
    if let Some(info) = video_infos.get_mut(info) {
        info.views += 1;
    }
}

async fn play_random_video_raw(state: web::Data<State>) -> impl Responder {
    let video_infos = state.video_info.read().unwrap();
    let index = state.random.borrow_mut().gen_range(0..video_infos.len());
    let entry = video_infos.get_index(index);
    if let Some(entry) = entry {
        HttpResponse::TemporaryRedirect()
            .insert_header(("Location", String::from("/files/video/") + entry.0))
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    } else {
        error!(state.lgr_important.borrow(), "Index does not exist"; "index" => index);
        HttpResponse::TemporaryRedirect()
            .insert_header(("Location", "/"))
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    }
}

async fn play_random_video(state: web::Data<State>) -> impl Responder {
    let video_infos = state.video_info.read().unwrap();
    let index = state.random.borrow_mut().gen_range(0..video_infos.len());
    let entry = video_infos.get_index(index);
    if let Some(entry) = entry {
        HttpResponse::TemporaryRedirect()
            .insert_header(("Location", String::from("/") + entry.0))
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    } else {
        error!(state.lgr_important.borrow(), "Index does not exist"; "index" => index);
        HttpResponse::TemporaryRedirect()
            .insert_header(("Location", "/"))
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    }
}

fn find_next_video(state: &web::Data<State>, path: &web::Path<String>) -> String {
    let video_infos = state.video_info.read().unwrap();
    let entry = video_infos.get_full(&**path);
    if let Some(entry) = entry {
        let index = entry.0;
        if let Some(entry) = video_infos.get_index(index + 1) {
            entry.0.into()
        } else if let Some(entry) = video_infos.get_index(0) {
            entry.0.into()
        } else {
            "".into()
        }
    } else {
        "".into()
    }
}

async fn play_next_video(path: web::Path<String>) -> impl Responder {
    HttpResponse::TemporaryRedirect()
        .insert_header(("Location", String::from("/") + &path))
        .cookie(
            Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_NEXT_VALUE)
                .path("/")
                .finish(),
        )
        .finish()
}

fn find_playmode(request: HttpRequest) -> PlayMode {
    if let Some(cookie) = request.cookie(COOKIE_NAME) {
        if cookie.value() == COOKIE_AUTOPLAY_RANDOM_VALUE {
            return PlayMode::Random;
        } else if cookie.value() == COOKIE_AUTOPLAY_NEXT_VALUE {
            return PlayMode::Sequential;
        }
    }
    PlayMode::default()
}

fn sorted_by_date(_ka: &String, va: &VideoInfo, _kb: &String, vb: &VideoInfo) -> cmp::Ordering {
    vb.added.cmp(&va.added)
}

fn time_ago(
    seconds: u64,
    limit: u64,
    singular: &'static str,
    plural: &'static str,
) -> Option<(u32, &'static str)> {
    if seconds > limit {
        let elapsed = seconds / limit;
        if elapsed == 1 {
            Some((elapsed as u32, singular))
        } else {
            Some((elapsed as u32, plural))
        }
    } else {
        None
    }
}

macro_rules! return_if_some {
    ($e:expr) => {{
        let tmp = $e;
        if let Some(x) = tmp {
            return x;
        }
    }};
}

fn compute_time_ago(now: SystemTime, then: SystemTime) -> (u32, &'static str) {
    match now.duration_since(then) {
        Ok(duration) => {
            let secs = duration.as_secs();
            return_if_some!(time_ago(
                secs,
                (3600.0 * 24.0 * 365.25) as u64,
                "year ago",
                "years ago"
            ));
            return_if_some!(time_ago(
                secs,
                (3600.0 * 24.0 * 30.44) as u64,
                "month ago",
                "months ago"
            ));
            return_if_some!(time_ago(secs, 3600 * 24 * 7, "week ago", "weeks ago"));
            return_if_some!(time_ago(secs, 3600 * 24, "day ago", "days ago"));
            return_if_some!(time_ago(secs, 3600, "hour ago", "hours ago"));
            return_if_some!(time_ago(secs, 60, "minute ago", "minutes ago"));
            (0, "Just now")
        }
        Err(_) => (0, "File is newer than current time"),
    }
}

fn generate_list_page(state: &mut State) {
    let video_infos = state.video_info.read().unwrap();
    let video_infos_clone_date = video_infos.clone();

    let html = html! {
        (DOCTYPE)
        html {
            head {
                (header_list())
                title { "All " (PLURALITY) " - " (LIST_TITLE) }
            }
            body {
                div class="boardBanner" {
                    div id="bannerCnt" class="title desktop" data-src="/files/images/banner.png" {
                        img alt=(NAME) src="/files/images/banner.png";
                    }
                    div class="boardTitle" { (format!("{} - {}", BOARD, NAME)) }
                }
                div class="navLinks mobile" {
                    span class="mobileib button" { a href=(format!("https://disqus.com/home/forum/{}/", FORUM_NAME)) { "View All Comments" } }
                    span class="mobileib button" { a href="/random" title="Redirects to a random Gondola" { "Random" } }
                    span class="mobileib button" { a href="/random-raw" title="Redirects to a random Gondola video stream" { "Random Raw" } }
                    span class="mobileib button" { a href="#bottom" { "Bottom" } }
                }
                hr class="desktop";
                div class="navLinks desktop" {
                    "[" a href=(format!("https://disqus.com/home/forum/{}/", FORUM_NAME)) { "View All Comments" } "]"
                    "[" a href="/random" title="Redirects to a random Gondola" { "Random" } "]"
                    "[" a href="/random-raw" title="Redirects to a random Gondola video stream" { "Random Raw" } "]"
                    "[" a href="#bottom" { "Bottom" } "]"
                }
                hr;
                h4 class="center" {
                    "Videos can be looped in most browsers: right-click + loop" br; "Videos normally autoplay." br; "If you click Next (ordered) autoplay will play  sequentially, if you click Next (random) autoplay will play in random order." br;
                    strong { "Gondola suggestions: " } (EMAIL)
                }
                h4 class="center" {
                    "There are " span class="rainbow-block" { (video_infos.len()) } " " (PLURALITY) " in this archive. "
                    span class="rainbow-block" {
                        ({
                            let mut count = 0;
                            for (_, video_info) in &*video_infos {
                                if video_info.source.is_some() {
                                    count += 1;
                                }
                            }
                            format!["{:.2}%", (count * 100) as f32 / video_infos.len() as f32]
                        })
                    }
                    " of " (PLURALITY) " have a source."
                }
                table id="arc-list" class="flashListing sortable" {
                    thead {
                        tr {
                            td class="postblock" { "Gondola Name" }
                            td class="postblock" { "Source" }
                            td class="postblock" { "Views" }
                            td class="postblock" { "Date Added" }
                            td class="postblock" { "Ago" }
                        }
                    }
                    tbody {
                        @for (video_name, video_info) in video_infos_clone_date.sorted_by(sorted_by_date) {
                            tr {
                                td { a href=(video_name) { (video_name) } }
                                td { (video_info.source.as_ref().unwrap_or(&String::new())) }
                                td { (video_info.views) }
                                td { ({
                                    let datetime: DateTime<Utc> = video_info.added.into();
                                    datetime.format("%A, %B %d, %Y %T")
                                }) }
                                td { ({
                                    let ago = compute_time_ago(SystemTime::now(), video_info.added);
                                    if ago.0 == 0 {
                                        ago.1.to_string()
                                    } else {
                                        format!["{} {}", ago.0, ago.1]
                                    }
                                })
                                }
                            }
                        }
                    }
                }
                hr;
                div class="navLinks navLinksBot desktop" {
                    "[" a href=(format!("https://disqus.com/home/forum/{}/", FORUM_NAME)) { "View All Comments" } "]"
                    "[" a href="/random" title="Redirects to a random Gondola" { "Random" } "]"
                    "[" a href="/random-raw" title="Redirects to a random Gondola video stream" { "Random Raw" } "]"
                    "[" a href="#top" { "Top" } "]"
                }
                hr class="desktop";
                div class="navLinks mobile" {
                    span class="mobileib button" { a href=(format!("https://disqus.com/home/forum/{}/", FORUM_NAME)) { "View All Comments" } }
                    span class="mobileib button" { a href="/random" title="Redirects to a random Gondola" { "Random" } }
                    span class="mobileib button" { a href="/random-raw" title="Redirects to a random Gondola video stream" { "Random Raw" } }
                    span class="mobileib button" { a href="#top" { "Top" } }
                }
                hr class="mobile";

                div class="cssDropdown" {
                    span class="stylechanger" {
                        "Style: "
                        select id="swapCSS" onchange="swapCSS()" {
                            option value="/files/css/yotsuba.css" { "Yotsuba" }
                            option value="/files/css/yotsublue.css" { "Yotsuba Blue" }
                        }
                    }
                }

                div id="bottom" {}
                script type="text/javascript" {
                    (PreEscaped("function swapCSS() { var x = document.getElementById(\"swapCSS\").value; document.getElementById(\"pageStyle\").setAttribute(\"href\", x); }"))
                }
                script type="text/javascript" src="/files/js/sorttable.js" {}
            }
        }
    };

    *state.listpage.write().unwrap() = html.into_string();
}

async fn list_all_videos(state: web::Data<State>) -> impl Responder {
    let listpage = state.listpage.read().unwrap();
    HttpResponse::Ok().body(listpage.clone())
}

async fn render_video_page(
    state: web::Data<State>,
    info: web::Path<String>,
    request: HttpRequest,
) -> impl Responder {
    let next_video = find_next_video(&state, &info);

    let play_mode = find_playmode(request);

    let path = String::from("/files/video/") + &info;

    increment_view_count(&state, &info);

    let video_infos = state.video_info.read().unwrap();
    let default_video_info = VideoInfo::default();
    let video_info = video_infos.get(&*info).unwrap_or(&default_video_info);
    let video_count = video_infos.len();
    let announcement = state.announcement.read().unwrap();

    let html = html! {
        (DOCTYPE)
        html {
            head {
                (header(state.style_count.load(Ordering::Relaxed)))
                title { (info) }
                script type="text/javascript" {
                    (PreEscaped("var forum_url = \"")) (FORUM_NAME) (PreEscaped("\";"))
                    (PreEscaped("var random_url = \"/random\";"))
                    (PreEscaped("var next_url = \"/next/")) (next_video) (PreEscaped("\";"))
                    "var play_random = " @if play_mode == PlayMode::Random { "true" } @else { "false" } ";"
                }
            }
            body class="main" {
                @if let Some(announcement) = &*announcement {
                    div class="announcement" {
                        (PreEscaped(announcement))
                    }
                }
                div class="video" {
                    video id="video" width="100%" height="100%" autoplay="true" onclick="toggle_pause();" onvolumechange="store_volume();" controls="" {
                        source src=(&path) type="video/webm";
                    }
                }
                script type="text/javascript" src="/files/js/video.js" {}
                div class="bottom" {
                    a class="button" href="/random" {
                        div class="center" {
                            span class="small" {
                                "Source: ";
                                br;
                                (video_info.source.as_ref().unwrap_or(&"Unknown (let me know in the comments)".to_string()));
                            }
                            br;
                            "Next (random)";
                            @if play_mode == PlayMode::Random {
                                br;
                                span class="autoplay" { "autoplaying random" }
                            }
                        }
                    }
                    a class="button" href=(&(String::from("/next/") + &next_video)) {
                        div class="center" {
                            span class="small" {
                                (next_video)
                                br;
                            }
                            "Next (ordered)";
                            @if play_mode == PlayMode::Sequential {
                                br;
                                span class="autoplay" { "autoplaying sequential" }
                            }
                        }
                    }
                    div class="button" onclick="show_comments();"{
                        div class="center" {
                            (video_info.views) " views";
                            br;
                            "Show "
                            a id="disqus_comments" href=(&(String::from("") + SITE_NAME + "/" + &*info + "#disqus_thread")) {
                                span class="loading" { "" }
                                " Comments"
                            }
                        }
                    }
                    a class="button" href="/list"{
                        div class="center" {
                            (video_count) " Webms";
                            br;
                            "Show All/Info";
                        }
                    }
                }
                div id="disqus_thread" hidden="";
                script type="text/javascript" src="files/js/disqus.js" {}
                script async="" id="dsq-count-scr" src=(&(String::from("//") + FORUM_NAME + ".disqus.com/count.js")) {}
                noscript { "Please enable Javascript to view the " a href="https://disqus.com/?ref_noscript" { "comments powered by Disqus." } }
            }
        }
    };
    HttpResponse::Ok().body(html.into_string())
}

async fn unknown_route(state: web::Data<State>, request: HttpRequest) -> impl Responder {
    let request_string = format!("{:#?}", request);
    info!(state.lgr.borrow(), "Unknown route accessed"; "request" => request_string);
    HttpResponse::TemporaryRedirect()
        .insert_header(("Location", "/"))
        .finish()
}

// ---

#[derive(Clone, Deserialize)]
struct ShellCommandForm {
    act: String,
    key: String,
}

enum RanState {
    NoCommandToRun,
    WrongPassword,
    PasswordNotHex,
    RanCommand(String),
}

pub fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}

async fn do_shell(state: web::Data<State>, form: web::Form<ShellCommandForm>) -> impl Responder {
    let ran_command;

    if !form.act.is_empty() && !form.key.is_empty() {
        let act_clone = form.act.clone();
        info!(state.lgr.borrow(), "Running shell"; "act" => act_clone);
        if let Ok(password) = slurp(&PathBuf::from("password")) {
            let password = password.trim();
            if let Ok(pw) = decode_hex(password) {
                let mut hasher = Sha512::new();
                hasher.update(form.key.as_bytes());
                let result = hasher.finalize();
                // println!("key: {:?}, result: {:?}, password: {:?}", form.key.as_bytes(), &result[..], decode_hex(password));
                if &result[..] == pw {
                    let string;
                    let act = form.act.clone();
                    if act == "style" {
                        state.style_count.fetch_add(1, Ordering::Relaxed);
                        string = "Style count increment";
                    } else if act == "denounce" {
                        *state.announcement.write().unwrap() = None;
                        string = "Announcement disabled";
                    } else if let Some(index) = form.act.find(' ') {
                        *state.announcement.write().unwrap() =
                            Some(form.act[index + 1..].to_string());
                        string = "Announcement changed";
                    } else {
                        string = "Unknown command";
                    }
                    ran_command = RanState::RanCommand(string.to_string());
                } else {
                    ran_command = RanState::WrongPassword;
                }
            } else {
                ran_command = RanState::PasswordNotHex;
            }
        } else {
            error!(
                state.lgr_important.borrow(),
                "Unable to read password file for shell commands"
            );
            ran_command = RanState::WrongPassword;
        }
    } else {
        ran_command = RanState::NoCommandToRun;
    }

    shell_render(ran_command, &form.key)
}

async fn shell() -> impl Responder {
    shell_render(RanState::NoCommandToRun, "")
}

fn shell_render(ran_command: RanState, key: &str) -> impl Responder {
    let html = html! {
        (DOCTYPE)
        html {
            head {
                (header(0))
                title { "Interactive Shell" }
            }
            body {
                p { "announce <whatever string here> - Excluding the < and > Will bring up a red bar on the main page with your announcement (Note that this string is NOT HTML escaped)" }
                p { "denounce - Will remove the announcement" }
                p { "style - Will increment the style counter so style updates are shown to users" }
                form action="shell" method="POST" {
                    input autofocus="" name="act" type="text" placeholder="Command" size="100";
                    br;
                    input name="key" type="password" placeholder="Key" value=(key);
                    br;
                    input type="submit" value="Submit";
                }
                br;
                a href="/" { "Return" }
                br;
                pre {
                    @match ran_command {
                        RanState::NoCommandToRun => {
                            "No command run"
                        }
                        RanState::WrongPassword => {
                            "Wrong password"
                        }
                        RanState::PasswordNotHex => {
                            "Password is not in hex format on the server"
                        }
                        RanState::RanCommand(string) => {
                            "Command executed:\n"
                            pre class="feedback" {
                                (&string)
                            }
                        }
                    }
                }
            }
        }
    };
    HttpResponse::Ok().body(html.into_string())
}

// ---

async fn redirect_favicon() -> impl Responder {
    HttpResponse::PermanentRedirect()
        .insert_header(("Location", "/files/favicon/128.png"))
        .finish()
}

async fn robots() -> impl Responder {
    HttpResponse::PermanentRedirect()
        .insert_header(("Location", "/files/misc/robots.txt"))
        .finish()
}

// ---

#[derive(Clone, Copy, Debug, PartialEq)]
enum PlayMode {
    Random,
    Sequential,
}

impl Default for PlayMode {
    fn default() -> Self {
        PlayMode::Random
    }
}

#[derive(Clone)]
struct State {
    pub announcement: Arc<RwLock<Option<String>>>,
    pub style_count: Arc<AtomicU64>,
    pub lgr: RefCell<Logger<Generic>>,
    pub lgr_important: RefCell<Logger<Generic>>,
    pub listpage: Arc<RwLock<String>>,
    pub random: RefCell<Random>,
    pub random_counter: Arc<AtomicU64>,
    pub video_info: Arc<RwLock<IndexMap<String, VideoInfo>>>,
}

impl Default for State {
    fn default() -> Self {
        let lgr =
            Logger::spawn_with_writer("site", writer::create_rotational_writer("files/logs/log"));
        let lgr_important = Logger::spawn_with_writer(
            "important",
            writer::create_rotational_writer("files/logs/important"),
        );
        lgr.set_colorize(true);
        lgr.set_log_level(LOGLEVEL);
        lgr_important.set_colorize(true);
        lgr_important.set_log_level(LOGLEVEL_IMPORTANT);
        Self {
            announcement: Arc::new(RwLock::new(None)),
            style_count: Arc::new(AtomicU64::new(0)),
            lgr: RefCell::new(lgr),
            lgr_important: RefCell::new(lgr_important),
            listpage: Arc::new(RwLock::new(String::new())),
            random: RefCell::new(Random::new(0)),
            random_counter: Arc::new(AtomicU64::new(0)),
            video_info: Arc::new(RwLock::new(IndexMap::new())),
        }
    }
}

#[derive(Clone, Debug)]
struct VideoInfo {
    pub added: SystemTime,
    pub source: Option<String>,
    pub views: usize,
}

impl Default for VideoInfo {
    fn default() -> Self {
        Self {
            added: SystemTime::UNIX_EPOCH,
            source: Option::default(),
            views: usize::default(),
        }
    }
}

// ---

fn slurp(path: &PathBuf) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn read_state_from_disk(state: &mut State) -> io::Result<()> {
    let lgr = state.lgr.borrow();
    let lgr_important = state.lgr_important.borrow();

    let directory = read_dir("files/video/")?;
    let mut video_infos = state.video_info.write().unwrap();

    for file in directory {
        let file = file?;
        let path = file.path();

        if let Some(Some(filename)) = path.file_name().map(|x| x.to_str()) {
            if filename.starts_with('.') {
                continue;
            }

            let modified = if let Ok(metadata) = file.metadata() {
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)
            } else {
                SystemTime::UNIX_EPOCH
            };

            let source: PathBuf = ["files", "sources", filename].iter().collect();
            let views: PathBuf = ["files", "statistics", filename].iter().collect();

            if let Ok(views) = slurp(&views).unwrap_or_else(|_| "0".into()).parse() {
                let video_info = VideoInfo {
                    added: modified,
                    source: slurp(&source).ok(),
                    views,
                };

                video_infos.insert(filename.into(), video_info.clone());

                let filename = String::from(filename);
                trace!(lgr, "Inserting file into table"; "filename" => filename, "info" => InDebug(&video_info); clone video_info);
            } else {
                error!(lgr_important, "Views file contains a non-number value"; "filename" => InDebug(&views));
            }
        } else {
            error!(lgr_important, "Unable to read file name from file"; "filename" => InDebug(&path));
        }
    }

    video_infos.sort_keys();
    Ok(())
}

fn update_state(mut state: State) {
    let lgr = state.lgr.borrow().clone_with_context("state-updater");
    let lgr_important = state.lgr_important.borrow().clone_add_context("important");
    loop {
        thread::sleep(Duration::from_secs(60 * 30));
        {
            benchmark! {
                |duration| info!(lgr, "Time to load video files and sources"; "duration" => InDebug(&duration)),
                match read_dir("files/video/") {
                    Ok(directory) => {
                        for file in directory {

                            let file = if let Ok(file) = file { file } else { continue };
                            let path = file.path();

                            if let Some(Some(filename)) = path.file_name().map(|x| x.to_str()) {

                                if filename.starts_with('.') {
                                    continue;
                                }

                                let modified = if let Ok(metadata) = file.metadata() {
                                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)
                                } else {
                                    SystemTime::UNIX_EPOCH
                                };

                                let source: PathBuf = ["files", "sources", filename].iter().collect();

                                let video_info = VideoInfo {
                                    added: modified,
                                    source: slurp(&source).ok(),
                                    views: 0,
                                };

                                {
                                    let mut writer = state.video_info.write().unwrap();
                                    if let Some(entry) = writer.get_mut(filename) {
                                        entry.added = video_info.added;
                                        entry.source = video_info.source;
                                    } else {
                                        writer.insert(filename.into(), video_info.clone());

                                        let filename = String::from(filename);
                                        trace!(lgr, "Inserting new file into table"; "filename" => filename, "info" => InDebug(&video_info); clone video_info);
                                    }
                                }
                            } else {
                                error!(lgr_important, "Unable to read file name from file"; "filename" => InDebug(&path));
                            }
                        }
                    }
                    Err(err) => {
                        error!(lgr_important, "Unable to read directory"; "directory" => "files/video", "error" => err);
                    }
                }
            }

            let video_infos = benchmark! {
                |duration| info!(lgr, "Time to copy table"; "duration" => InDebug(&duration)),
                state.video_info.read().unwrap().clone()
            };

            benchmark! {
                |duration| info!(lgr, "Time to write statistics to disk"; "duration" => InDebug(&duration)),
                for (key, value) in video_infos.iter() {
                    let views: PathBuf = ["files", "statistics", &key].iter().collect();
                    match File::create(views) {
                        Ok(mut file) => {
                            match file.write_all(value.views.to_string().as_bytes()) {
                                Ok(_) => {}
                                Err(err) => {
                                    error!(lgr_important, "Unable to write to statistics file"; "error" => err);
                                }
                            }
                        }
                        Err(err) => {
                            error!(lgr_important, "Unable to create statistics file"; "error" => err);
                        }
                    }

                }
            }
        }
        generate_list_page(&mut state);
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mut state = State::default();
    read_state_from_disk(&mut state)?;
    generate_list_page(&mut state);

    let updater_state = state.clone();
    thread::Builder::new()
        .name("state-updater".to_string())
        .spawn(move || {
            update_state(updater_state);
        })
        .expect("Unable to start the updater thread");

    info!(state.lgr.borrow(), "Initializing"; "working directory" => InDebug(&std::env::current_dir()));

    HttpServer::new(move || {
        let seed = state.random_counter.fetch_add(1, Ordering::Relaxed);
        let mut thread_state = state.clone();
        thread_state.random = RefCell::new(Random::new((1_103_515_245 * seed + 12345) as u128));

        info!(thread_state.lgr.borrow(), "Starting worker thread"; "random seed" => seed);

        let benchmark_log = state.lgr.borrow().clone_with_context("benchmark");
        let request_log = state.lgr.borrow().clone_with_context("request");

        App::new()
            .app_data(Data::new(thread_state))
            .wrap_fn(move |req, srv| {
                let request = format!("{:?}", req);
                info!(request_log, "Incoming request"; "data" => request);
                benchmark! {
                    |duration| info!(benchmark_log, "Total request time"; "duration" => InDebug(&duration)),
                    srv.call(req)
                }
            })
            .route("/", web::get().to(index))
            .route("/random", web::get().to(play_random_video))
            .route("/random-raw", web::get().to(play_random_video_raw))
            .route("/next/{previous}", web::get().to(play_next_video))
            .route("/robots.txt", web::get().to(robots))
            .route("/list", web::get().to(list_all_videos))
            .route("favicon.ico", web::get().to(redirect_favicon))
            .route("/files/{filename:.*}", web::get().to(get_file))
            .route("/shell", web::get().to(shell))
            .route("/shell", web::post().to(do_shell))
            .route("/{name}", web::get().to(render_video_page))
            .default_service(web::get().to(unknown_route))
    })
    .bind(format!("127.0.0.1:{}", PORT))?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_sequences() {
        let mut random = Random::new(0);
        let mut random1 = random.clone();
        let mut random2 = Random::new(random.gen());

        for _ in 0..100 {
            assert_ne!(random1.gen::<usize>(), random2.gen::<usize>());
        }
    }
}
