window.addEventListener("load", function() { window. scrollTo(0, 0); });

document.getElementById('video').addEventListener('ended', ended, false);
function ended(handle) {
    if (play_random) {
      history.pushState({
        prevUrl: window.location.href
      }, 'Next page', random_url);
    } else {
      history.pushState({
        prevUrl: window.location.href
      }, 'Next page', next_url);
    }
    history.go();
}

function toggle_pause() {
    let f = navigator.userAgent.search("Firefox");
    if (f == -1) {
        return;
    }

    if (document.getElementById('video').paused) {
       document.getElementById('video').play();
    } else {
       document.getElementById('video').pause();
    }
}

function store_volume() {
    if (typeof(Storage) !== "undefined") {
        localStorage.setItem("volume", document.getElementById('video').volume);
    }
}

if (typeof(Storage) !== "undefined") {
    var last_volume = localStorage.getItem("volume");
    if (last_volume !== null && last_volume != 0) {
        document.getElementById('video').volume = last_volume;
    }
}

var showing = false;
function show_comments(handle) {
    if (showing) {
      document.getElementById('disqus_thread').style.display = 'none';
    } else {
      if (!loaded_disqus) {
        load_disqus();
        loaded_disqus = true;
      }
      document.getElementById('disqus_thread').style.display = 'block';
    }
    showing = !showing;
}
