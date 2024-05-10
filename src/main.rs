#![allow(non_snake_case)]
use clap::Parser;
use log::{error, info};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::Path;

#[derive(Parser)]
#[clap(name = "DLsiteRenamer", version = "0.4.3.1", author = "Jade")]
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
    /// TODO: 考虑换成XPath选择器，相关crate: Skyscraper
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
    circle_name_selector_str: r#"#work_maker td .maker_name a"#,
    full_url_pattern: r#"https://www.dlsite.com/pro/work/=/product_id/{id}.html"#,
};

const BJ_WORK: Work = Work {
    title_regex_string: r"^BJ\d+$",
    pid_selector_str: r#".work_right_info"#,
    wname_selector_str: r#"#work_name"#,
    // 如果使用Skyscraper的话，相应XPath选择器
    // 应该是：//th[contains(text(), '著者')]/following-sibling::td[1]
    circle_name_selector_str: r#"#work_maker > tbody:nth-child(1) > tr:nth-child(1) > td:nth-child(2) > a:nth-child(1)"#,
    full_url_pattern: r#"https://www.dlsite.com/books/work/=/product_id/{id}.html"#,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    my_logger::init();

    let v = read_directories_from_file("settings.json").expect("读取 settings.json 失败!");
    // 使用正则表达式匹配以RJ开头后面跟一个或多个数字
    let rj_regex = regex::Regex::new(RJ_WORK.title_regex_string).unwrap();
    let vj_regex = regex::Regex::new(VJ_WORK.title_regex_string).unwrap();
    let bj_regex = regex::Regex::new(BJ_WORK.title_regex_string).unwrap();

    let mut match_found_flag = false;

    let delimiter = delimiter();
    for dir in &v {
        let entries_result = fs::read_dir(dir);

        let mut entries: Vec<_> = match entries_result {
            Ok(entries) => entries.filter_map(Result::ok).map(|e| e.path()).collect(),
            Err(e) => {
                error!("读取目录失败：{dir}，错误：{}", e);
                continue; // 在这里使用continue，跳过当前迭代
            }
        };

        if entries.is_empty() {
            error!("读取目录失败：{dir}，请检查目录是否存在或者是否有权限访问！");
            continue;
        }

        entries.sort();

        let filtered_entries: Vec<_> = entries
            .into_iter()
            .filter(|e| {
                // 获取文件名，不含扩展名
                let file_stem = e.file_stem().unwrap().to_str().unwrap();
                // 检查文件名是否匹配正则表达式
                let matches_rj_pattern = rj_regex.is_match(file_stem);
                let matches_vj_pattern = vj_regex.is_match(file_stem);
                let matches_bj_pattern = bj_regex.is_match(file_stem);
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
                        && (matches_rj_pattern || matches_vj_pattern || matches_bj_pattern)
                        && not_ends_with_disallowed_extension
                } else {
                    // 仅文件模式关闭时，包含文件和文件夹
                    (matches_rj_pattern || matches_vj_pattern || matches_bj_pattern)
                        && not_ends_with_disallowed_extension
                }
            })
            .collect();

        if !filtered_entries.is_empty() {
            match_found_flag = true;
        } else {
            continue;
        }

        for e in filtered_entries {
            let opath = e.display();
            if cfg!(debug_assertions) {
                dbg!(&opath);
            }
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

    if !match_found_flag {
        info!("未发现任何匹配项！");
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
    } else if regex::Regex::new(BJ_WORK.title_regex_string)
        .unwrap()
        .is_match(&id)
    {
        workname_selector = Selector::parse(BJ_WORK.wname_selector_str).unwrap();
        pid_selector = Selector::parse(BJ_WORK.pid_selector_str).unwrap();
        circle_name_selector = Selector::parse(BJ_WORK.circle_name_selector_str).unwrap();
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
    let document = Html::parse_document(&text); // TODO: 如果使用Skyscraper，这里需要另外创建一个Skyscraper专用的Document

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
    } else if regex::Regex::new(BJ_WORK.title_regex_string)
        .unwrap()
        .is_match(&id)
    {
        page_url = BJ_WORK.full_url_pattern.replace(r"{id}", &id);
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
