#[test]
fn test_cqstr_to_arr() {


    use crate::mytool::str_msg_to_arr;
    let js = serde_json::json!("hello,world[CQ:image,file=xxx.png]hello,world");
    let ret = str_msg_to_arr(&js).unwrap();
    println!("test_cqstr_to_arr ret:`{}`",ret.to_string());
}


#[test]
fn test_cqparse() {
    use std::collections::BTreeMap;
    let data_str = "[CQ:image,file=620a6c143114a4feaaf9e89cc83162b6.image,subType=0,url=https://gchat.qpic.cn/]";
    let pos1 = data_str.find(",").ok_or("CQ码解析失败").unwrap();
    let tp = data_str.get(4..pos1).ok_or("CQ码解析失败").unwrap();
    let mut sub_key_obj:BTreeMap<String,String> = BTreeMap::new();
    sub_key_obj.insert("type".to_string(), tp.to_string());
    let re = fancy_regex::Regex::new("[:,]([^\\[\\],]+?)=([^\\[\\],]*?)(?=[\\],])").unwrap();

    for cap_iter in re.captures_iter(&data_str) {
        let cap = cap_iter.unwrap();
        let len = cap.len();
        if len == 3 {
            let key = &cap[1];
            let val = &cap[2];
            let key = key.replace("&#91;", "[");
            let key = key.replace("&#93;", "]");
            let key = key.replace("&#44;", ",");
            let key = key.replace("&amp;", "&");
            let val = val.replace("&#91;", "[");
            let val = val.replace("&#93;", "]");
            let val = val.replace("&#44;", ",");
            let val = val.replace("&amp;", "&");
            sub_key_obj.insert(key, val);
        }
    }
    println!("{:?}",sub_key_obj);
}

#[test]
fn test_redformat() {
    // fn is_black_char(ch: char) -> bool {
    //     ch == ' ' || ch == '\r' || ch == '\n' || ch == '\t'
    // }
    let test_str = "【定义变量@使用情况数组@【数组@0@0@0】】";
    let content = test_str.chars().collect::<Vec<char>>();
    let mut out_content = String::new();
    let mut index = 0;
    while index < content.len() {
        if content[index] != '【' {
            out_content.push(content[index]);
            index += 1;
            continue;
        }
        else {
            let next_char = content.get(index + 1).ok_or("syntax error").unwrap();
            if next_char.to_owned() == '@' {
                let mut num = 1;
                for index2 in index..content.len() {
                    if content[index2] == '【' {
                        num += 1;
                    }
                    else if content[index2] == '】' {
                        num -= 1;
                    }
                    if num == 0 {
                        let s = content.get(index..index2).unwrap();
                        out_content.push_str(&String::from_iter(s.iter()));
                        index = index2 + 1;
                        break;
                    }
                }
                if num != 0 {
                    break;
                }
            } else {

            }
        }

    }

}