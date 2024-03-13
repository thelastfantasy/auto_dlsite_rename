#![allow(non_snake_case)]
use chrono::Local;
use clap::Parser;
use env_logger::Builder as LoggerBuilder;
use log::{error, info};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::Path;

#[derive(Parser)]
#[clap(name = "DLsiteRenamer", version = "0.4.2", author = "Jade")]
#[clap(about = "本程序将自动读取你的预设目录，并重命名其中DLsite番号文件和文件夹为实际作品标题，部分作品使用了非法的标点符号或者路径长度超出系统限制会重命名失败，请手动重命名", long_about = None)]
struct Cli {
    /// 仅文件模式开关，默认关闭（处理文件和文件夹）
    #[clap(short = 'f', long = "file-only", default_value_t = false)]
    file_only: bool,
}

struct Work {
    title_regex_string: &'static str,
    pid_selector_str: &'static str,
    wname_selector_str: &'static str,
    circle_name_selector_str: &'static str,
    full_url_pattern: &'static str,
}

const RJ_WORK: Work = Work {
    title_regex_string: r#"^RJ\d+$"#,
    pid_selector_str: r#".work_right_info"#,
    wname_selector_str: r#"#work_name"#,
    circle_name_selector_str: r#"#work_maker td a"#,
    full_url_pattern: r#"https://www.dlsite.com/maniax/work/=/product_id/{id}.html"#,
};

const VJ_WORK: Work = Work {
    title_regex_string: r"^VJ\d+$",
    pid_selector_str: r#".work_right_info"#,
    wname_selector_str: r#"#work_name"#,
    circle_name_selector_str: r#"#work_maker td a"#,
    full_url_pattern: r#"https://www.dlsite.com/pro/work/=/product_id/{id}.html"#,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    LoggerBuilder::new()
        .format(|buf, record| {
            let level_style = buf.default_level_style(record.level());
            let reset = level_style.render_reset();
            let level_style = level_style.render();

            // let error_style = buf.default_level_style(log::Level::Error);
            // let error_reset = error_style.render_reset();
            // let error_style = error_style.render();

            let now = Local::now();

            let message = format!("{}", record.args());

            if message.trim().is_empty() {
                // 如果消息体为空，则输出一个空行或自定义的空消息格式
                writeln!(buf)
            } else {
                // 否则，按常规方式输出日志消息
                writeln!(
                    buf,
                    "{} {} {level_style}{}{reset}  - {}",
                    now.format("%Y-%m-%d"),
                    now.format("%H:%M:%S"),
                    format!("[{}]", record.level()),
                    message
                )
            }
        })
        .filter_level(log::LevelFilter::Info)
        .init();

    let v = read_directories_from_file("settings.json").expect("读取 settings.json 失败!");
    // 使用正则表达式匹配以RJ开头后面跟一个或多个数字
    let rj_regex = regex::Regex::new(RJ_WORK.title_regex_string).unwrap();
    let vj_regex = regex::Regex::new(VJ_WORK.title_regex_string).unwrap();
    let delimiter = delimiter();
    for dir in &v {
        let mut entries = fs::read_dir(dir)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, io::Error>>()?;
        entries.sort();
        let entries = entries.into_iter().filter(|e| {
            // 获取文件名，不含扩展名
            let file_stem = e.file_stem().unwrap().to_str().unwrap();
            // 检查文件名是否匹配正则表达式
            let matches_rj_pattern = rj_regex.is_match(file_stem);
            let matches_vj_pattern = vj_regex.is_match(file_stem);
            // 检查扩展名是否不以以下扩展名结尾
            let extension = e.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let disallowed_extensions = vec!["!qt", "!ut", "downloading"];
            let not_ends_with_disallowed_extension = !disallowed_extensions
                .iter()
                .any(|disallowed_extension| extension == *disallowed_extension);
            // 检查是否是文件夹
            let is_dir = e.is_dir();

            if cli.file_only {
                // 仅文件模式开启时，仅包含文件
                !is_dir
                    && (matches_rj_pattern || matches_vj_pattern)
                    && not_ends_with_disallowed_extension
            } else {
                // 仅文件模式关闭时，包含文件和文件夹
                (matches_rj_pattern || matches_vj_pattern) && not_ends_with_disallowed_extension
            }
        });

        for e in entries {
            let opath = e.display();
            let pid = e.file_stem().unwrap().to_str().unwrap();
            let fut = dlsite_req(pid);
            let page_url = get_page_url(pid);
            match fut.await {
                Ok(fut) => {
                    let output = fut;
                    let parent = e.parent().unwrap().to_str().unwrap();
                    let ext = e.extension().and_then(|ext| ext.to_str()).unwrap_or("");
                    if ext != "" {
                        let result = fs::rename(&e, format!("{parent}{delimiter}{output}.{ext}"));
                        match result {
                            Ok(_) => {
                                info!("");
                                info!("{opath} => 发现匹配文件，开始重命名...\n{output}.{ext}");
                                info!("");
                            }
                            Err(e) => {
                                error!("");
                                error!("已跳过：{opath} ，原因：{pid} {e}");
                                error!("请访问作品URL以手动重命名：{page_url}");
                                error!("");
                                continue;
                            }
                        }
                    } else {
                        let result = fs::rename(&e, format!("{parent}{delimiter}{output}"));
                        match result {
                            Ok(_) => {
                                info!("");
                                info!("{opath} => 发现匹配文件，开始重命名...\n{output}");
                                info!("");
                            }
                            Err(e) => {
                                error!("");
                                error!("已跳过：{opath} ，原因：{pid} {e}");
                                error!("请访问作品URL以手动重命名：{page_url}");
                                error!("");
                                continue;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("已跳过：{opath} ，原因：{pid} {e}");
                    continue;
                }
            }
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
    let id = id.to_uppercase();

    let page_url = get_page_url(&id);
    let workname_selector;
    let pid_selector;
    let circle_name_selector;

    if regex::Regex::new(RJ_WORK.title_regex_string)
        .unwrap()
        .is_match(&id)
    {
        workname_selector = Selector::parse(RJ_WORK.wname_selector_str).unwrap();
        pid_selector = Selector::parse(RJ_WORK.pid_selector_str).unwrap();
        circle_name_selector = Selector::parse(RJ_WORK.circle_name_selector_str).unwrap();
    } else if regex::Regex::new(VJ_WORK.title_regex_string)
        .unwrap()
        .is_match(&id)
    {
        workname_selector = Selector::parse(VJ_WORK.wname_selector_str).unwrap();
        pid_selector = Selector::parse(VJ_WORK.pid_selector_str).unwrap();
        circle_name_selector = Selector::parse(VJ_WORK.circle_name_selector_str).unwrap();
    } else {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::Other,
            "未知的作品ID格式！",
        )));
    };

    let resp = reqwest::get(page_url).await?;
    // info!("{}", resp.url().to_string());

    let text = resp.text().await?;

    let mut filename: HashMap<&str, String> = HashMap::new();
    let document = Html::parse_document(&text);

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

    let pid = if let Some(pid) = filename.get("pid") {
        pid.to_string()
    } else {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::Other,
            "产品ID未找到！",
        )));
    };
    let wname = if let Some(wname) = filename.get("wname") {
        wname.to_string()
    } else {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::Other,
            "workname not found",
        )));
    };
    let cname = filename.get("cname").unwrap().trim();

    let filename = format!("[{pid}][{cname}] {wname}")
        .replace("/", "／")
        .replace("\\", "＼")
        .replace("?", "？")
        .replace("*", "＊")
        .replace("<", "＜")
        .replace(">", "＞")
        .replace("|", "｜")
        .replace("\"", "")
        .replace(":", "：")
        .replace("&", "＆")
        .replace("!", "！");

    Ok(filename.trim().to_string())
}

fn get_page_url(id: &str) -> String {
    let id = id.to_uppercase();

    let page_url;

    if regex::Regex::new(RJ_WORK.title_regex_string)
        .unwrap()
        .is_match(&id)
    {
        page_url = RJ_WORK.full_url_pattern.replace(r"{id}", &id);
    } else if regex::Regex::new(VJ_WORK.title_regex_string)
        .unwrap()
        .is_match(&id)
    {
        page_url = VJ_WORK.full_url_pattern.replace(r"{id}", &id);
    } else {
        return "Invalid_Work_Id".to_string();
    };

    page_url
}

#[cfg(target_os = "windows")]
fn delimiter() -> &'static str {
    "\\"
}

#[cfg(not(target_os = "windows"))]
fn delimiter() -> &'static str {
    "/"
}
