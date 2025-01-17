use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fs;
use std::panic;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use cqapi::cq_get_app_directory2;
use httpserver::init_http_server;

use mytool::read_json_str;
use redlang::RedLang;
use serde_json;
use rust_embed::RustEmbed;

use cqapi::cq_add_log_w;
use cqapi::cq_get_app_directory1;


mod cqapi;
mod cqevent;
mod redlang;
mod mytool;
mod initevent;
mod cronevent;
mod botconn;
mod httpserver;
mod httpevent;
mod pyserver;
mod test;

#[macro_use]
extern crate lazy_static; 

#[derive(Clone,Debug)]
pub struct ScriptInfo {
    pkg_name:String,
    script_name:String
}

pub struct InputStream {
    pub self_id:String,
    pub group_id:String,
    pub user_id:String,
    pub guild_id:String,
    pub channel_id:String,
    pub echo:String,
    pub stream_type:String,
    pub tx:Option<Arc<Mutex<std::sync::mpsc::Sender<String>>>>
}

#[derive(Debug)]
pub struct ScriptRelatMsg {
    pub self_id:String,
    pub msg_id_vec:Vec<String>,
    pub create_time:u64
}

lazy_static! {
    // 用于记录加载的脚本
    pub static ref G_SCRIPT:RwLock<serde_json::Value> = RwLock::new(serde_json::json!([]));
    // 用于记录加载的包名
    pub static ref G_PKG_NAME:RwLock<HashSet<String>> = RwLock::new(HashSet::new());
    // 用于类型UUID
    pub static ref REDLANG_UUID:String = uuid::Uuid::new_v4().to_string();
    // 用于分页命令
    pub static ref PAGING_UUID:String = uuid::Uuid::new_v4().to_string();
    // 用于清空命令
    pub static ref CLEAR_UUID:String = uuid::Uuid::new_v4().to_string();
    // 用于记录常量:包名-常量名-常量值
    pub static ref G_CONST_MAP:RwLock<HashMap<String,HashMap<String, String>>> = RwLock::new(HashMap::new());
    // 用于撤回消息
    pub static ref G_MSG_ID_MAP:RwLock<HashMap<String,VecDeque<String>>> = RwLock::new(HashMap::new());
    // 用于记录自定义的命令
    pub static ref G_CMD_MAP:RwLock<HashMap<String,HashMap<String, String>>> = RwLock::new(HashMap::new());
    // 用于记录命令
    pub static ref G_CMD_FUN_MAP:RwLock<HashMap<String, fn(&mut RedLang,&[String]) -> Result<Option<String>, Box<dyn std::error::Error>>>> = RwLock::new(HashMap::new());
    // 异步事件循环
    pub static ref  RT_PTR:Arc<tokio::runtime::Runtime> = Arc::new(tokio::runtime::Runtime::new().unwrap());
    // 退出标记
    pub static ref G_QUIT_FLAG:RwLock<bool> = RwLock::new(false);
    // 记录正在运行的脚本数量（用于退出）
    pub static ref G_RUNNING_SCRIPT_NUM:RwLock<usize> = RwLock::new(0usize);
    // 记录正在运行的脚本名字
    pub static ref G_RUNNING_SCRIPT:RwLock<Vec<ScriptInfo>> = RwLock::new(vec![]);
    // 输入流记录
    pub static ref G_INPUTSTREAM_VEC:RwLock<Vec<InputStream>> = RwLock::new(vec![]);
    // webui的访问密码
    pub static ref G_WEB_PASSWORD:RwLock<Option<String>> = RwLock::new(None);
    // webui的访问密码2
    pub static ref G_READONLY_WEB_PASSWORD:RwLock<Option<String>> = RwLock::new(None);
    // 全局锁
    pub static ref G_LOCK:Mutex<HashMap<String,HashMap<String, i32>>> = Mutex::new(HashMap::new());
    // 记录与某条消息相关的脚本输出
    pub static ref G_SCRIPT_RELATE_MSG:RwLock<HashMap<String,ScriptRelatMsg>> = RwLock::new(HashMap::new());
}



#[derive(RustEmbed)]
#[folder = "res/"]
#[prefix = "res/"]
pub struct Asset;


#[derive(RustEmbed)]
#[folder = "doc/"]
#[prefix = "doc/"]
pub struct AssetDoc;

pub fn wait_for_quit() -> ! {
    (*G_QUIT_FLAG.write().unwrap()) = true;
    let _foo = std::thread::spawn(||{
        std::thread::sleep(core::time::Duration::from_secs(5));
        cq_add_log_w("退出软件超时(5s)，强制退出!").unwrap();
        let running_scripts = get_running_script_info();
        cq_add_log_w(&format!("未退出脚本:{:?}",running_scripts)).unwrap();
        std::process::exit(-1);
    });
    loop {
        {
            if (*G_RUNNING_SCRIPT_NUM.read().unwrap()) == 0 {
                break;
            }
        }
        std::thread::sleep(core::time::Duration::from_millis(1));
    }
    std::process::exit(0);
}

pub fn add_running_script_num(pkg_name:&str,script_name:&str) -> bool {
    if *G_QUIT_FLAG.read().unwrap() == true {
        return false;
    }
    let mut lk = G_RUNNING_SCRIPT_NUM.write().unwrap();
    (*lk) += 1;
    let mut lk = G_RUNNING_SCRIPT.write().unwrap();
    lk.push(ScriptInfo {
        pkg_name: pkg_name.to_owned(),
        script_name: script_name.to_owned()
    });
    return true;
}

pub fn get_running_script_info() -> Vec<ScriptInfo> {
    let lk = G_RUNNING_SCRIPT.read().unwrap();
    let mut ret_vec:Vec<ScriptInfo> = vec![];
    for i in 0..lk.len() {
        let script_info = lk.get(i).unwrap();
        ret_vec.push((*script_info).clone());
    }
    return ret_vec;
}

pub fn dec_running_script_num(pkg_name:&str,script_name:&str) {
    let mut lk = G_RUNNING_SCRIPT_NUM.write().unwrap();
    if (*lk) != 0 {
        (*lk) -= 1;
    }
    let mut lk = G_RUNNING_SCRIPT.write().unwrap();
    let mut pos = 0;
    let mut isfind = false;
    for i in 0..lk.len() {
        let script_info = lk.get(i).unwrap();
        if script_info.script_name == script_name && pkg_name == script_info.pkg_name {
            pos = i;
            isfind = true;
            break;
        }
    }
    if isfind {
        lk.remove(pos);
    }
}


// 这是插件第一个被调用的函数，不要在这里调用任何CQ的API,也不要在此处阻塞
pub fn initialize() -> i32 {
    cq_add_log_w(&format!("欢迎使用`红色问答{}`,正在进行资源初始化...",get_version())).unwrap();
    panic::set_hook(Box::new(|e| {
        cq_add_log_w(e.to_string().as_str()).unwrap();
    }));

    // 初始化配置文件
    init_config();

    redlang::webexfun::init_web_ex_fun_map();
    redlang::cqexfun::init_cq_ex_fun_map();
    redlang::exfun::init_ex_fun_map();
    redlang::init_core_fun_map();

    if let Err(err) = release_file(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = init_http_server(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    // 创建python运行环境
    std::thread::spawn(||{
        if let Err(err) = init_python(){
            cq_add_log_w(&err.to_string()).unwrap();
        }
    });
    
    if let Err(err) = init_code(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    if let Err(err) = botconn::do_conn_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = cronevent::do_cron_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    cq_add_log_w("资源初始化完成！").unwrap();
    return 0;
}

pub fn read_config() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let script_path = cq_get_app_directory1()? + "config.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{
        let config_json = serde_json::json!({
            "web_port":1207,
            "web_password":"",
            "readonly_web_password":"",
            "web_host":"127.0.0.1",
            "ws_urls":[],
            "not_open_browser":false
        });
        fs::write(script_path.clone(), config_json.to_string()).unwrap();
    }
    let script = fs::read_to_string(script_path)?;
    Ok(serde_json::from_str(&script)?)
}

fn create_python_env() -> Result<(), Box<dyn std::error::Error>> {
    let app_dir = cq_get_app_directory1()?;
    fs::create_dir_all(app_dir.clone() + "pymain")?;
    let foo = std::process::Command::new("python").current_dir(app_dir).arg("-m").arg("venv").arg("pymain").status();
    if foo.is_err() {
        return Err(RedLang::make_err(&format!("python环境创建失败:{:?}",foo)));
    }else {
        cq_add_log_w(&format!("python服务创建:{:?}",foo.unwrap())).unwrap();
    }
    Ok(())
}

pub fn init_python() -> Result<(), Box<dyn std::error::Error>> {
    create_python_env()?;
    let config = read_config()?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    let code = r#"
import json
import threading
import time
import os

lk = threading.Lock()
WS_APP = None

def red_install(pkg_name):
    '''安装一个模块'''
    from pip._internal.cli import main
    ret = main.main(['install', pkg_name, '-i',
                    'https://pypi.tuna.tsinghua.edu.cn/simple', "--no-warn-script-location"])

    if ret != 0:
        err = "安装依赖{}失败".format(pkg_name)
        raise Exception(err)

def deal_msg_t(message):
    try:
        deal_msg(message)
    except Exception as e:
        print(e)

def deal_msg(message):
    js = json.loads(message)
    echo = js["echo"]
    code = js["code"]
    code = """
__red_out_data = ""
def red_install(pkg_name):
    '''安装一个模块'''
    from pip._internal.cli import main
    ret = main.main(['install', pkg_name, '-i',
                    'https://pypi.tuna.tsinghua.edu.cn/simple', "--no-warn-script-location"])

    if ret != 0:
        err = "安装依赖{}失败".format(pkg_name)
        raise Exception(err)
def red_in():
    return __red_in_data
def red_out(s):
    global __red_out_data
    __red_out_data = s
""" + code
    input_t = js["input"]
    scope = {"__red_in_data":input_t}
    exec(code,scope)
    to_send = {"echo":echo,"data":scope["__red_out_data"]}
    lk.acquire()
    try:
        WS_APP.send(json.dumps(to_send))
    finally:
        lk.release()

def on_message(_, message):
    threading.Thread(target=deal_msg,args=(message,)).start()

def on_open(_):
    WS_APP.send("opened")

def conn_fun():
    global WS_APP
    WS_APP = websocket.WebSocketApp(
        "ws://127.0.0.1:"+os.environ.get('port', '1207')+"/pyserver",
        on_message=on_message,
        on_open= on_open,
        cookie="password={}".format(os.environ.get('password', ''))
    )
    WS_APP.run_forever()

red_install("websocket-client")
import websocket
conn_fun()
"#;
    let password:String = url::form_urlencoded::byte_serialize(read_web_password()?.as_bytes()).collect();
    let curr_env = std::env::var("PATH").unwrap_or_default();
    
    let new_env = if cfg!(target_os = "windows") {
        format!("{}pymain/Scripts;{}",cq_get_app_directory1()?,curr_env)
    } else {
        format!("{}pymain/bin:{}",cq_get_app_directory1()?,curr_env)
    };

    thread::spawn(move ||{
        let foo = std::process::Command::new("python")
        .env("PATH", new_env)
        .arg("-c")
        .arg(code)
        .env("port", port.to_string())
        .env("password", password)
        .status();
        if foo.is_err() {
            cq_add_log_w(&format!("python服务启动失败:{:?}",foo)).unwrap();
        }else {
            cq_add_log_w(&format!("python服务退出:{:?}",foo.unwrap())).unwrap();
        }
    });
    Ok(())
}

pub fn init_config() {
    let script_path = cq_get_app_directory1().unwrap() + "config.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone()).unwrap().is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{

        let mut line1 = String::new();
        println!("请输入端口号(默认1207):");
        std::io::stdin().read_line(&mut line1).unwrap();
        let web_port;
        if line1.trim() == "" {
            web_port = 1207;
        }else {
            web_port = line1.trim().parse::<u16>().unwrap();
        }
        
        let mut line2 = String::new();
        let web_host:&str;
        println!("请输入主机地址(默认127.0.0.1):");
        std::io::stdin().read_line(&mut line2).unwrap();
        if line2.trim() == "" {
            web_host = "127.0.0.1";
        } else {
            web_host = &line2.trim();
        }

        let config_json = serde_json::json!({
            "web_port":web_port,
            "web_password":"",
            "readonly_web_password":"",
            "web_host":web_host,
            "ws_urls":[],
            "not_open_browser":false
        });
        fs::write(script_path.clone(), config_json.to_string()).unwrap();
    }
}

pub fn read_web_password() -> Result<String, Box<dyn std::error::Error>> {
    {
        let lk = G_WEB_PASSWORD.read()?;
        if lk.is_some() {
            return Ok(lk.clone().unwrap());
        }
    }
    let mut ret_str = String::new();
    let config = read_config()?;
    if let Some(pass_opt) = config.get("web_password") {
        if let Some(pass) = pass_opt.as_str() {
            ret_str = pass.to_string();
        }
    }
    {
        let mut lk = G_WEB_PASSWORD.write()?;
        *lk = Some(ret_str.clone());
    }
    return Ok(ret_str);
}

pub fn read_readonly_web_password() -> Result<String, Box<dyn std::error::Error>> {
    {
        let lk = G_READONLY_WEB_PASSWORD.read()?;
        if lk.is_some() {
            return Ok(lk.clone().unwrap());
        }
    }
    let mut ret_str = String::new();
    let config = read_config()?;
    if let Some(pass_opt) = config.get("readonly_web_password") {
        if let Some(pass) = pass_opt.as_str() {
            ret_str = pass.to_string();
        }
    }
    {
        let mut lk = G_READONLY_WEB_PASSWORD.write()?;
        *lk = Some(ret_str.clone());
    }
    return Ok(ret_str);
}

pub fn set_ws_urls(ws_urls:serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = read_config()?;
    config["ws_urls"] = ws_urls;
    let script_path = cq_get_app_directory1()? + "config.json";
    fs::write(script_path,config.to_string())?;
    Ok(())
}

pub fn release_file() -> Result<(), Box<dyn std::error::Error>> {
    let sep = std::path::MAIN_SEPARATOR;
    let err = "get asset err";
    fs::create_dir_all(cq_get_app_directory1().unwrap() + "webui")?;
    for it in Asset::iter() {
        let file = Asset::get(&it.to_string()).ok_or(err)?;
        fs::write(cq_get_app_directory1().unwrap() + "webui" + &sep.to_string() + it.to_string().get(4..).unwrap_or_default(), file.data)?;
    } 
    for it in AssetDoc::iter() {
        let file = AssetDoc::get(&it.to_string()).ok_or(err)?;
        fs::write(cq_get_app_directory1().unwrap() + "webui" + &sep.to_string() + it.to_string().get(4..).unwrap_or_default(), file.data)?;
    } 
    Ok(())
}


pub fn get_version() -> String {
    let file = Asset::get("res/version.txt").unwrap();
    let buf = file.data;
    let version_str = String::from_utf8(buf.to_vec()).unwrap();
    return version_str;
}

pub fn get_all_pkg_name_by_cache() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let wk = G_PKG_NAME.read()?;
    let mut ret: Vec<String> = vec![];
    for it in &*wk {
        ret.push(it.to_owned());
    }
    Ok(ret)
}


fn get_all_pkg_name() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let plus_dir_str = cq_get_app_directory1()?;
    let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
    std::fs::create_dir_all(&pkg_dir)?;
    let dirs = fs::read_dir(&pkg_dir)?;
    let mut pkg_names:Vec<String> = vec![];
    for dir in dirs {
        let path = dir?.path();
        if path.is_dir() {
            pkg_names.push(format!("{}",path.file_name().unwrap().to_string_lossy()));
        }
    }
    if pkg_names.contains(&"默认包".to_string()) {
        // 这里强制退出程序
        return Err(RedLang::make_err("附加包的包名不可以为`默认包`!")).unwrap();
    }
    Ok(pkg_names)
}

fn get_all_pkg_code() -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let plus_dir_str = cq_get_app_directory1()?;
    let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
    let pkg_names = get_all_pkg_name()?;
    let mut arr_val:Vec<serde_json::Value> = vec![];
    for it in &pkg_names {
        let script_path = pkg_dir.join(&it).join("script.json");
        {
            // 判断文件是否存在
            let mut is_file_exists = false;
            if fs::metadata(script_path.clone()).is_ok() {
                if fs::metadata(script_path.clone())?.is_file(){
                    is_file_exists = true;
                }
            }
            // 不存在就创建文件
            if !is_file_exists{
                fs::write(script_path.clone(), "[]")?;
            }
        }
        
        let script = fs::read_to_string(script_path)?;
        let mut pkg_script_vec:Vec<serde_json::Value> = serde_json::from_str(&script)?;
        for js in &mut pkg_script_vec {
            if let Some(obj) = js.as_object_mut() {
                obj.insert("pkg_name".to_string(),serde_json::Value::String(it.to_string()));
                arr_val.push(serde_json::Value::Object(obj.clone()));
            }
        }
    }
    Ok(arr_val)
}

pub fn init_code() -> Result<(), Box<dyn std::error::Error>>{
    let script_path = cq_get_app_directory2()? + "script.json";
    // 判断文件是否存在
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    // 不存在就创建文件
    if !is_file_exists{
        fs::write(script_path.clone(), "[]")?;
    }

    // 获取默认包代码
    let script = fs::read_to_string(script_path)?;
    let mut arr_val:Vec<serde_json::Value> = serde_json::from_str(&script)?;

    // 获取所有三方包代码
    let pkg_codes = get_all_pkg_code()?;
    for it in pkg_codes {
        arr_val.push(it);
    }

    // 保存代码到内存
    {
        let mut wk = G_SCRIPT.write()?;
        (*wk) = serde_json::Value::Array(arr_val);
    }

    {
        // 刷新包名
        let pkg_names = get_all_pkg_name()?;
        let mut lk = G_PKG_NAME.write()?;
        lk.clear();
        for it in &pkg_names {
            lk.insert(it.to_owned());
        }
    }

    // 执行初始化脚本
    if let Err(err) = initevent::do_init_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    Ok(())
}


pub fn save_code(contents: &str) -> Result<(), Box<dyn std::error::Error>> {

    // 解析网络数据
    let mut code_map:HashMap<String,Vec<serde_json::Value>> = HashMap::new();
    let mut key_vec:Vec<String> = vec![];
    let mut rename_pkg_process:Vec<Vec<String>> = vec![];
    let js:Vec<serde_json::Value>; 

    {
        let js_t:Vec<serde_json::Value> = serde_json::from_str(contents)?;
        js  = js_t.get(2..).ok_or("save_code err 1")?.to_vec();

        // 获得脚本内容
        { 
            for it in &js {
                let mut it_t = it.to_owned();
                // 得到网络包的包名,如果没有pkg_name，则默认为"",网络包中的默认包是没有pkg_name的
                let pkg_name_str = read_json_str(&it_t, "pkg_name");
                it_t.as_object_mut().ok_or("it_t not obj")?.remove("pkg_name");
                if !code_map.contains_key(&pkg_name_str) {
                    code_map.insert(pkg_name_str.to_owned(), vec![]);
                }
                code_map.get_mut(&pkg_name_str).unwrap().push(it_t);
            }
        }

        // 获得存在的包
        for it in js_t.get(1).unwrap().as_array().ok_or("save_code err 2")? {
            let s = it.as_str().ok_or("save_code err 3")?;
            key_vec.push(s.to_owned());
        }

        // 获得重命名目录的方案
        for it in js_t.get(0).unwrap().as_array().ok_or("save_code err 4")? {
            let k1 = it.as_array().ok_or("save_code err 5")?.get(0).ok_or("save_code err 6")?.as_str().ok_or("save_code err 7")?.to_owned();
            let k2 = it.as_array().ok_or("save_code err 5")?.get(1).ok_or("save_code err 6")?.as_str().ok_or("save_code err 7")?.to_owned();
            rename_pkg_process.push(vec![k1,k2]);
        }
    }
    

    // 保存脚本
    {
        let plus_dir_str = cq_get_app_directory1()?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");

        // 修改文件名
        for it in rename_pkg_process {
            fs::rename(pkg_dir.join(it[0].to_owned()), pkg_dir.join(it[1].to_owned()))?;
        }

        // 创建文件夹
        for pkg_name in &key_vec {
            let script_path = pkg_dir.join(pkg_name);
            std::fs::create_dir_all(&script_path)?;
        }

        for (pkg_name,code) in &code_map {
            let cont = serde_json::Value::Array(code.to_vec()).to_string();
            if pkg_name == "" {
                fs::write(cq_get_app_directory2()? + "script.json", cont)?;
            }else { 
                let script_path = pkg_dir.join(pkg_name);
                std::fs::create_dir_all(&script_path)?;
                fs::write(script_path.join("script.json"), cont)?;
            }
        }

        // 删除目录下多余的包
        let pkg_names = get_all_pkg_name()?;
        for name in &pkg_names {
            if !key_vec.contains(name) {
                let script_path = pkg_dir.join(name);
                let _ = fs::remove_dir_all(script_path);
            }
        }

    }
   
    // 重新加载脚本
    if let Err(err) = crate::init_code() {
        cq_add_log_w(&format!("can't call init evt:{}",err)).unwrap();
    }
    Ok(())
}

pub fn read_code_cache() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let wk = G_SCRIPT.read()?;
    Ok((*wk).clone())
}