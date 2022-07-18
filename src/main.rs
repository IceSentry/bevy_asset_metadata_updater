use cargo_toml::Package;
use reqwest::header::USER_AGENT;
use serde_derive::{Deserialize, Serialize};
use url::Url;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize)]
struct AssetMetadata {
    name: String,
    description: String,
    link: String,
    bevy_versions: Option<Vec<String>>,
    licenses: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct GithubContentResponse {
    encoding: String,
    content: String,
}

fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;

    use reqwest::blocking::Client;
    let client = Client::new();

    let args: Vec<String> = std::env::args().collect();
    for entry in WalkDir::new(&args[1]) {
        let entry = entry?;
        if entry.path().is_dir() {
            continue;
        }

        if entry.path().extension().unwrap() != "toml" {
            continue;
        }

        println!("Updating {:?}", entry.path().display());

        let file = std::fs::read_to_string(entry.path())?;
        let asset_metadata = match toml::from_str::<AssetMetadata>(&file) {
            Ok(it) => it,
            Err(err) => {
                eprintln!("{err}");
                continue;
            }
        };

        let mut asset_metadata = asset_metadata;
        let url = Url::parse(&asset_metadata.link)?;
        if url.host_str() != Some("github.com") {
            continue;
        }

        let segments = url.path_segments().map(|c| c.collect::<Vec<_>>()).unwrap();
        let username = segments[0];
        let repository_name = segments[1];

        let response = client
            .get(format!(
                "https://api.github.com/repos/{username}/{repository_name}/contents/Cargo.toml"
            ))
            .header("Accept", "application/json")
            .header(USER_AGENT, "bevy-tools")
            .bearer_auth(std::env::var("GITHUB_TOKEN")?)
            .send();

        let response = match response {
            Ok(it) => it,
            Err(err) => {
                eprintln!("{err}");
                continue;
            }
        };

        let json: GithubContentResponse = match response.json() {
            Ok(it) => it,
            Err(err) => {
                eprintln!("{err}");
                continue;
            }
        };

        // The github rest api is supposed to return the content as a base64 encoded string
        let content = if json.encoding == "base64" {
            String::from_utf8(base64::decode(json.content.replace('\n', "").trim())?)?
        } else {
            eprintln!("content is not in base64");
            continue;
        };

        let cargo_toml = toml::from_str::<cargo_toml::Manifest>(&content)?;

        if let Some(Package {
            license: Some(license),
            ..
        }) = cargo_toml.package
        {
            asset_metadata.licenses =
                Some(license.split("OR").map(|x| x.trim().to_string()).collect());
        }

        // Find any dep that starts with bevy and get the version
        // This makes sure to handle all the bevy_* crates
        let version = cargo_toml
            .dependencies
            .keys()
            .find(|k| k.starts_with("bevy"))
            .and_then(|key| cargo_toml.dependencies.get(key).and_then(get_version));

        if let Some(version) = version {
            asset_metadata.bevy_versions = Some(vec![version]);
        }

        // Write the results to the asset file
        std::fs::write(entry.path(), toml::to_string(&asset_metadata)?.as_bytes())?;
    }
    Ok(())
}

fn get_version(dep: &cargo_toml::Dependency) -> Option<String> {
    match dep {
        cargo_toml::Dependency::Simple(version) => Some(version.to_string()),
        cargo_toml::Dependency::Detailed(detail) => {
            if let Some(version) = &detail.version {
                Some(version.to_string())
            } else if detail.git.is_some() && detail.branch == Some(String::from("main")) {
                Some(String::from("main"))
            } else {
                None
            }
        }
    }
}
