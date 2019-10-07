#![feature(proc_macro_hygiene)]
use actix_files::NamedFile;
use actix_service::Service;
use actix_web::{
    cookie::Cookie, http::header, web, App, HttpMessage, HttpRequest, HttpResponse, HttpServer, Responder,
};
use chrono::{prelude::*, DateTime};
use fast_logger::{error, info, trace, warn, Generic, InDebug, Logger};
use indexmap::IndexMap;
use maud::{html, Markup, PreEscaped, DOCTYPE};
use rand::Rng;
use rand_pcg::Pcg64Mcg as Random;
use std::{
    cell::RefCell,
    cmp,
    fs::{read_dir, remove_file, File},
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

// ---

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

static COOKIE_NAME: &'static str = "autoplay";
static COOKIE_AUTOPLAY_RANDOM_VALUE: &'static str = "random";
static COOKIE_AUTOPLAY_NEXT_VALUE: &'static str = "next";

static PLURALITY: &'static str = "Gondolas";
static LIST_TITLE: &'static str = "GondolaArchive";
static DEFAULT_VIDEO: &'static str = "/FrontPage.webm";
static DESCRIPTION: &'static str = "Gondola webms depicting our favorite silent observer";
static SINGULAR: &'static str = "Gondola";
static FORUM_NAME: &'static str = "evo-1";
static SITE_NAME: &'static str = "http://gondola.stravers.net";

fn header() -> Markup {
    html! {
        meta charset="UTF-8";
        meta name="viewport" content="width=device-width,maximum-scale=1,minimum-scale=1,minimal-ui";
        link rel="icon" type="image/png" href="/files/favicon/16.png";
        link rel="icon" type="image/png" href="/files/favicon/32.png";
        link rel="icon" type="image/png" href="/files/favicon/64.png";
        link rel="icon" type="image/png" href="/files/favicon/128.png";
        link rel="stylesheet" type="text/css" href="/files/css/reset.css";
        link rel="stylesheet" type="text/css" href="/files/css/style.css?x=4";
        meta name="description" content=(DESCRIPTION);
        meta property="og:title" content=(SINGULAR);
        meta property="og:description" content=(DESCRIPTION);
        meta property="og:image" content="/files/favicon/128.png";
    }
}

// ---

fn index() -> impl Responder {
    HttpResponse::PermanentRedirect()
        .set_header("Location", DEFAULT_VIDEO)
        .finish()
}

fn get_file(state: web::Data<State>, req: HttpRequest) -> actix_web::Result<NamedFile> {
    let mut path = PathBuf::from("files/");
    let rest = req
        .match_info()
        .query("filename")
        .parse::<PathBuf>()
        .unwrap();
    path.push(&rest);
    match NamedFile::open(path) {
        Ok(file) => Ok(file),
        Err(err) => {
            warn![state.lgr_important.borrow_mut(), "Request for non-existent file"; "filename" => InDebug(&rest)];
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

fn play_random_video_raw(state: web::Data<State>) -> impl Responder {
    let video_infos = state.video_info.read().unwrap();
    let index = state.random.borrow_mut().gen_range(0, video_infos.len());
    let entry = video_infos.get_index(index);
    if let Some(entry) = entry {
        HttpResponse::TemporaryRedirect()
            .set_header("Location", String::from("/files/video/") + entry.0)
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    } else {
        error![state.lgr_important.borrow_mut(), "Index does not exist"; "index" => index];
        HttpResponse::TemporaryRedirect()
            .set_header("Location", "/")
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    }
}

fn play_random_video(state: web::Data<State>) -> impl Responder {
    let video_infos = state.video_info.read().unwrap();
    let index = state.random.borrow_mut().gen_range(0, video_infos.len());
    let entry = video_infos.get_index(index);
    if let Some(entry) = entry {
        HttpResponse::TemporaryRedirect()
            .set_header("Location", String::from("/") + entry.0)
            .cookie(
                Cookie::build(COOKIE_NAME, COOKIE_AUTOPLAY_RANDOM_VALUE)
                    .path("/")
                    .finish(),
            )
            .finish()
    } else {
        error![state.lgr_important.borrow_mut(), "Index does not exist"; "index" => index];
        HttpResponse::TemporaryRedirect()
            .set_header("Location", "/")
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

fn play_next_video(path: web::Path<String>) -> impl Responder {
    HttpResponse::TemporaryRedirect()
        .set_header("Location", String::from("/") + &path)
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

fn sorted_by_views(_ka: &String, va: &VideoInfo, _kb: &String, vb: &VideoInfo) -> cmp::Ordering {
    vb.views.cmp(&va.views)
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
        if tmp.is_some() {
            return tmp.unwrap();
        }
    }};
}

fn compute_time_ago(now: SystemTime, then: SystemTime) -> (u32, &'static str) {
    match now.duration_since(then) {
        Ok(duration) => {
            let secs = duration.as_secs();
            return_if_some![time_ago(
                secs,
                (3600.0 * 24.0 * 365.25) as u64,
                "year ago",
                "years ago"
            )];
            return_if_some![time_ago(
                secs,
                (3600.0 * 24.0 * 30.44) as u64,
                "month ago",
                "months ago"
            )];
            return_if_some![time_ago(secs, 3600 * 24 * 7, "week ago", "weeks ago")];
            return_if_some![time_ago(secs, 3600 * 24, "day ago", "days ago")];
            return_if_some![time_ago(secs, 3600, "hour ago", "hours ago")];
            return_if_some![time_ago(secs, 60, "minute ago", "minutes ago")];
            (0, "Just now")
        }
        Err(_) => (0, "File is newer than current time"),
    }
}

fn generate_list_page(state: &mut State) {
    let video_infos = state.video_info.read().unwrap();
    let video_infos_clone = video_infos.clone();
    let video_infos_clone_date = video_infos.clone();

    let mut total_views = 0;
    for (_, video_info) in video_infos.iter() {
        total_views += video_info.views;
    }

    let html = html! {
        html {
            (DOCTYPE)
            head {
                (header())
                title { "All " (PLURALITY) " - " (LIST_TITLE) }
            }
            body {
                a href="/files/archive/gondolas.zip" { "Download All (zip file)" }
                p {
                    a href="https://disqus.com/home/forum/evo-1/" { "All comments on this site!" }
                }
                p {
                    strong { "Public" } " API: " a href="/random" { "/random" } " redirects to a random " (SINGULAR) ". "
                    a href="/random-raw" { "/random-raw" } " redirects to a random " (SINGULAR) " video stream."
                }
                p {
                    strong { "Videos" } " can be looped in most browsers: right-click -> loop"
                }
                p {
                    strong { "Videos" } " normally autoplay. If you click Next (ordered) autoplay will play sequentially, if you click Next (random) autoplay will play in random order."
                }
                p {
                    strong { (SINGULAR) } " suggestions: macocio@gmail.com"
                }
                br;
                p { "Recently added " (PLURALITY) }
                div class="small-scroll" {
                    table {
                        @for (video_name, video_info) in video_infos_clone_date.sorted_by(sorted_by_date) {
                            tr {
                                th { ({
                                    let ago = compute_time_ago(SystemTime::now(), video_info.added);
                                    if ago.0 == 0 {
                                        ago.1.to_string()
                                    } else {
                                        format!["{} {}", ago.0, ago.1]
                                    }
                                })
                                }
                                th { a href=(video_name) { (video_name) } }
                                th { ({
                                    let datetime: DateTime<Utc> = video_info.added.into();
                                    datetime.format("%A, %B %d, %Y %T")
                                }) }
                            }
                        }
                    }
                }
                br;
                p {
                    "There are " span class="rainbow-block" { (video_infos.len()) } " " (PLURALITY) " in this archive. "
                    span class="rainbow-block" {
                        ({
                            let mut count = 0;
                            for (_, video_info) in &*video_infos {
                                if video_info.source.is_some() {
                                    count += 1;
                                }
                            }
                            format!["{:.2}", (count * 100) as f32 / video_infos.len() as f32]
                        })
                        "%"
                    }
                    " of " (PLURALITY) " have a source."
                }
                br;
                table class="source-table" {
                    tr { th { (SINGULAR) " (by name)" } th { "Views" } th { "Source" } }
                    tr { th { "-------" } th { "-----" } th { "-----" } }
                    tr { th { "Total" } th { (total_views) } th { "" } }
                    tr { th { "-------" } th { "-----" } th { "-----" } }
                    @for (video_name, video_info) in video_infos.iter() {
                        tr { th { a href=(video_name) { (video_name) }} th { (video_info.views) } th { (video_info.source.as_ref().unwrap_or(&"".to_string())) }}
                    }
                }
                table class="view-table" {
                    tr { th { (SINGULAR) " (by views)" } th { "Views" } }
                    tr { th { "-------" } th { "-----" } }
                    tr { th { "Total" } th { (total_views) } }
                    tr { th { "-------" } th { "-----" } }
                    @for (video_name, video_info) in video_infos_clone.sorted_by(sorted_by_views) {
                        tr { th { a href=(video_name) { (video_name) }} th { (video_info.views) } }
                    }
                }
            }
        }
    };

    *state.listpage.write().unwrap() = html.into_string();
}

fn list_all_videos(state: web::Data<State>) -> impl Responder {
    let listpage = state.listpage.read().unwrap();
    HttpResponse::Ok().body(&*listpage)
}

fn render_video_page(
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

    let html = html! {
        (DOCTYPE)
        html {
            head {
                (header())
                title { (info) }
                script type="text/javascript" {
                    (PreEscaped("var forum_url = \"")) (FORUM_NAME) (PreEscaped("\";"))
                    (PreEscaped("var random_url = \"/random\";"))
                    (PreEscaped("var next_url = \"/next/")) (info) (PreEscaped("\";"))
                    "var play_random = " @if play_mode == PlayMode::Random { "true" } @else { "false" } ";"
                }
            }
            body class="main" {
                div class="announcement" {
                    "GondolaArchive has been rewritten from Racket to Rust. (Response 3 ms -> 25 µs, memory 214 MB -> 9.7 MB)";
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

fn unknown_route(state: web::Data<State>, request: HttpRequest) -> impl Responder {
    let request_string = format!["{:#?}", request];
    info![state.lgr.borrow_mut(), "Unknown route accessed"; "request" => request_string];
    HttpResponse::TemporaryRedirect()
        .set_header("Location", "/")
        .finish()
}

fn redirect_favicon() -> impl Responder {
    HttpResponse::PermanentRedirect()
        .set_header("Location", "/files/favicon/128.png")
        .finish()
}

fn robots() -> impl Responder {
    HttpResponse::PermanentRedirect()
        .set_header("Location", "/files/misc/robots.txt")
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
    pub lgr: RefCell<Logger<Generic>>,
    pub lgr_important: RefCell<Logger<Generic>>,
    pub listpage: Arc<RwLock<String>>,
    pub random: RefCell<Random>,
    pub random_counter: Arc<AtomicU64>,
    pub video_info: Arc<RwLock<IndexMap<String, VideoInfo>>>,
}

impl Default for State {
    fn default() -> Self {
        let mut lgr =
            Logger::spawn_with_writer("site", writer::create_rotational_writer("files/logs/log"));
        let mut lgr_important = Logger::spawn_with_writer(
            "important",
            writer::create_rotational_writer("files/logs/important"),
        );
        lgr.set_colorize(true);
        lgr.set_log_level(128);
        lgr_important.set_colorize(true);
        lgr_important.set_log_level(255);
        Self {
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
    let mut lgr = state.lgr.borrow_mut();
    let mut lgr_important = state.lgr_important.borrow_mut();

    let directory = read_dir("files/video/")?;
    let mut video_infos = state.video_info.write().unwrap();

    for file in directory {
        let file = file?;
        let path = file.path();

        if let Some(Some(filename)) = path.file_name().map(|x| x.to_str()) {
            if filename.chars().next().unwrap() == '.' {
                continue;
            }

            let modified = if let Ok(metadata) = file.metadata() {
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)
            } else {
                SystemTime::UNIX_EPOCH
            };

            let source: PathBuf = ["files", "sources", filename].iter().collect();
            let views: PathBuf = ["files", "statistics", filename].iter().collect();

            if let Ok(views) = slurp(&views).unwrap_or("0".into()).parse() {
                let video_info = VideoInfo {
                    added: modified,
                    source: slurp(&source).ok(),
                    views,
                };

                video_infos.insert(filename.into(), video_info.clone());

                let filename = String::from(filename);
                trace![lgr, "Inserting file into table"; "filename" => filename, "info" => InDebug(&video_info); clone video_info];
            } else {
                error![lgr_important, "Views file contains a non-number value"; "filename" => InDebug(&views)];
            }
        } else {
            error![lgr_important, "Unable to read file name from file"; "filename" => InDebug(&path)];
        }
    }

    video_infos.sort_keys();
    Ok(())
}

fn update_state(mut state: State) {
    let mut lgr = state.lgr.borrow().clone_with_context("state-updater");
    let mut lgr_important = state.lgr_important.borrow().clone_add_context("important");
    loop {
        thread::sleep(Duration::from_secs(60 * 30));
        {
            benchmark! {
                |duration| info![lgr, "Time to load video files and sources"; "duration" => InDebug(&duration)],
                match read_dir("files/video/") {
                    Ok(directory) => {
                        for file in directory {

                            let file = if let Ok(file) = file { file } else { continue };
                            let path = file.path();

                            if let Some(Some(filename)) = path.file_name().map(|x| x.to_str()) {

                                if filename.chars().next().unwrap() == '.' {
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
                                        trace![lgr, "Inserting new file into table"; "filename" => filename, "info" => InDebug(&video_info); clone video_info];
                                    }
                                }
                            } else {
                                error![lgr_important, "Unable to read file name from file"; "filename" => InDebug(&path)];
                            }
                        }
                    }
                    Err(err) => {
                        error![lgr_important, "Unable to read directory"; "directory" => "files/video", "error" => err];
                    }
                }
            }

            match read_dir("files/remove") {
                Ok(directory) => {
                    for file in directory {
                        let file = if let Ok(file) = file { file } else { continue };
                        let path = file.path();

                        if let Some(Some(filename)) = path.file_name().map(|x| x.to_str()) {
                            if filename.chars().next().unwrap() == '.' {
                                continue;
                            }

                            {
                                let mut writer = state.video_info.write().unwrap();
                                writer.swap_remove(filename);
                                let filename = filename.to_string();
                                trace![lgr, "Removing file from table"; "filename" => filename];
                            }

                            let filename = filename.to_string();
                            if let Err(err) = remove_file(path) {
                                error![lgr_important, "Unable to remove removal file"; "error" => err, "filename" => filename];
                            }
                        }
                    }
                }
                Err(err) => {
                    error![lgr_important, "Unable to read directory"; "directory" => "files/remove", "error" => err];
                }
            }

            let video_infos = benchmark! {
                |duration| info![lgr, "Time to copy table"; "duration" => InDebug(&duration)],
                state.video_info.read().unwrap().clone()
            };

            benchmark! {
                |duration| info![lgr, "Time to write statistics to disk"; "duration" => InDebug(&duration)],
                for (key, value) in video_infos.iter() {
                    let views: PathBuf = ["files", "statistics", &key].iter().collect();
                    match File::create(views) {
                        Ok(mut file) => {
                            match file.write_all(value.views.to_string().as_bytes()) {
                                Ok(_) => {}
                                Err(err) => {
                                    error![lgr_important, "Unable to write to statistics file"; "error" => err];
                                }
                            }
                        }
                        Err(err) => {
                            error![lgr_important, "Unable to create statistics file"; "error" => err];
                        }
                    }

                }
            }
        }
        generate_list_page(&mut state);
    }
}

fn main() -> std::io::Result<()> {
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

    info![state.lgr.borrow_mut(), "Initializing"; "working directory" => InDebug(&std::env::current_dir())];

    HttpServer::new(move || {
        let seed = state.random_counter.fetch_add(1, Ordering::Relaxed);
        let mut thread_state = state.clone();
        thread_state.random = RefCell::new(Random::new((1103515245 * seed + 12345) as u128));

        info![thread_state.lgr.borrow_mut(), "Starting worker thread"; "random seed" => seed];

        let mut benchmark_log = state.lgr.borrow_mut().clone_with_context("benchmark");
        let mut request_log = state.lgr.borrow_mut().clone_with_context("request");

        App::new()
            .data(thread_state)
            .wrap_fn(move |req, srv| {
                let request = format!["{:?}", req];
                info![request_log, "Incoming request"; "data" => request];
                benchmark! {
                    |duration| info![benchmark_log, "Total request time"; "duration" => InDebug(&duration)],
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
            .route("/{name}", web::get().to(render_video_page))
            .default_service(web::get().to(unknown_route))
    })
    .bind("127.0.0.1:8081")?
    .run()
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
            assert_ne![random1.gen::<usize>(), random2.gen::<usize>()];
        }
    }
}
