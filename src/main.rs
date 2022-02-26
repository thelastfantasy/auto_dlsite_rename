use regex_macro::regex;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let v = read_directories_from_file("settings.json").expect("读取 settings.json 失败!");
    for dir in &v {
        let mut entries = fs::read_dir(dir)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, io::Error>>()?;
        entries.sort();
        let entries = entries
            .into_iter()
            .filter(|e| regex!(r#"^RJ\d+$"#).is_match(e.file_stem().unwrap().to_str().unwrap()));
        for e in entries {
            let opath = e.display();
            let fut = dlsite_req(e.file_stem().unwrap().to_str().unwrap());
            let output = fut.await?;
            let parent = e.parent().unwrap().to_str().unwrap();
            let ext = e.extension().unwrap().to_str().unwrap();
            let _r = fs::rename(&e, format!("{parent}\\{output}.{ext}"));
            println!("{opath} =>发现匹配文件，开始重命名...\n{output}.{ext}");
        }
    }
    Ok(())
}

fn read_directories_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<String>, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let d = serde_json::from_reader(reader)?;

    Ok(d)
}

async fn dlsite_req(id: &str) -> Result<String, Box<dyn Error>> {
    let resp = reqwest::get(format!(
        "https://www.dlsite.com/maniax/work/=/product_id/{id}.html"
    ))
    .await?;
    // println!("{}", resp.url().to_string());

    let text = resp.text().await?;

    let mut filename: HashMap<&str, String> = HashMap::new();
    let document = Html::parse_document(&text);
    let workname_selector = Selector::parse(r#"#work_name"#).unwrap();
    let pid_selector = Selector::parse(r#".work_right_info"#).unwrap();
    let circle_name_selector = Selector::parse(r#"#work_maker td a"#).unwrap();
    for element in document.select(&workname_selector) {
        let wname = element.text().collect::<Vec<_>>().join("");
        filename.insert("wname", wname);
    }
    for element in document.select(&pid_selector) {
        let pid = element
            .value()
            .attr("data-product-id")
            .expect("Product ID not found")
            .to_string();
        filename.insert("pid", pid);
    }
    for element in document.select(&circle_name_selector) {
        let cname = element.text().collect::<Vec<_>>().join("");
        filename.insert("cname", cname);
    }

    let wname = filename.get("wname").unwrap();
    let pid = filename.get("pid").unwrap();
    let cname = filename.get("cname").unwrap().trim();

    let filename = format!("[{cname}][{pid}] {wname}")
        .replace("/", "／")
        .replace("\\", "＼")
        .replace("?", "？")
        .replace("*", "＊")
        .replace("<", "＜")
        .replace(">", "＞")
        .replace("|", "｜")
        .replace(":", "：")
        .replace("!", "！");

    Ok(filename.to_string())
}
