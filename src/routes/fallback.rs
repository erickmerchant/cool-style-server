use crate::args::Commands;
use anyhow::Result;
use axum::{
	body::{to_bytes, Body},
	extract::State,
	http::{header, Request, StatusCode},
	response::{IntoResponse, Response},
};
use camino::Utf8Path;
use hyper::header::HeaderValue;
use lol_html::{element, html_content::ContentType, HtmlRewriter, Settings};
use std::{fs, sync::Arc};
use url::Url;

pub async fn fallback_handler(
	State(state): State<Arc<crate::State>>,
	req: Request<Body>,
) -> Result<Response<Body>, crate::Error> {
	let (parts, body) = match state.args.command.clone() {
		Commands::Proxy {
			host,
			directory: _directory,
		} => {
			let path_and_query = req
				.uri()
				.path_and_query()
				.map(|pq| format!("{pq}").to_string())
				.unwrap_or("".to_string());
			let base_url = Url::parse(host.as_str())?;
			let url = base_url.join(path_and_query.as_str())?;
			let (parts, body) = req.into_parts();
			let mut req = hyper::Request::from_parts(parts, body);

			*req.uri_mut() = url.to_string().parse()?;
			req.headers_mut()
				.insert("accept-encoding", HeaderValue::from_str("identity")?);

			let res = state.client.request(req).await?.into_response();

			let (parts, body) = res.into_parts();

			(parts, body)
		}
		Commands::Serve { directory } => {
			let path = req.uri().path();
			let is_index = path.ends_with('/');
			let path = path.trim_start_matches('/');
			let mut path = Utf8Path::new(directory.as_str()).join(path);

			if is_index {
				path.push("index.html");
			}

			let mut res = StatusCode::NOT_FOUND.into_response();

			if let Some(content_type) = mime_guess::from_path(&path).first() {
				if let Ok(body) = fs::read(path) {
					res = (
						StatusCode::OK,
						[(
							header::CONTENT_TYPE,
							format!("{content_type}; charset=utf-8"),
						)],
						body,
					)
						.into_response();
				}
			};

			res.into_parts()
		}
	};

	let bytes = to_bytes(body, usize::MAX).await?;
	let mut res = Response::from_parts(parts.clone(), Body::from(bytes.clone())).into_response();

	if res
		.headers()
		.get("content-type")
		.map_or(false, |h| h.as_ref().starts_with("text/html".as_bytes()))
	{
		let mut output = Vec::new();
		let mut rewriter = HtmlRewriter::new(
			Settings {
				element_content_handlers: vec![
					element!("link[rel=stylesheet]", |el| {
						el.set_attribute("is", "cool-stylesheet")?;

						Ok(())
					}),
					element!("head", |el| {
						el.append(
							&format!(
								r#"<script type="module" src="/{}/cool-stylesheet.js"></script>"#,
								state.args.cool_base
							),
							ContentType::Html,
						);

						Ok(())
					}),
				],
				..Default::default()
			},
			|c: &[u8]| output.extend_from_slice(c),
		);

		rewriter.write(&bytes)?;
		rewriter.end()?;

		let body = String::from_utf8(output)?;

		res.headers_mut().remove("content-length");
		*res.body_mut() = Body::from(body);
	}

	Ok(res)
}