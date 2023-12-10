use async_stream::try_stream;
use axum::{
	extract::State,
	response::sse::{Event, KeepAlive, Sse},
};
use camino::Utf8Path;
use futures::{channel::mpsc::channel, executor::block_on, stream::Stream, SinkExt, StreamExt};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use pathdiff::diff_paths;
use serde_json::json;
use std::{convert::Infallible, fs::canonicalize, path, sync::Arc, time::Duration};

pub async fn watch_handler(
	State(state): State<Arc<crate::State>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
	Sse::new(try_stream! {
		let (mut tx, mut rx) = channel(1);
		let mut watcher = RecommendedWatcher::new(
			move |res| {
				block_on(async {
					tx.send(res).await.expect("should send");
				})
			},
			Config::default(),
		).expect("watcher should be created");

		watcher.watch(path::Path::new(state.args.watch.as_str()), RecursiveMode::Recursive).expect("watcher should watch");

		while let Some(res) = rx.next().await {
			match res {
				Ok(event) => {
					let hrefs : Vec<Option<String>> = event.paths.iter().map(|p| {
						let c = canonicalize(state.args.watch.as_str()).expect("path should be valid");

						diff_paths(p, c).map(|p| {
							let p = p.to_str().expect("path should be a string");
							let base = Utf8Path::new(state.args.style_base.as_str());
							let p = base.join(p);

							format!("/{p}")
						})
					}).collect();

					yield Event::default().data(json!({
						"hrefs": hrefs,
					}).to_string());
				},
				Err(e) => println!("watch error: {:?}", e),
			}
		}
	})
	.keep_alive(
		KeepAlive::new()
			.interval(Duration::from_secs(10))
			.text("keep-alive-text"),
	)
}
