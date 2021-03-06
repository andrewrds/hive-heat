use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::env;
use reqwest::blocking::Client;
use serde_json::{Value, map::Map};
use http::header::{HeaderValue, AUTHORIZATION};
use toml::Value as TomlValue;
use dirs;

const HIVE_API_ENDPOINT: &str = "https://beekeeper.hivehome.com/1.0/";
 
fn main() {
    let args: Vec<String> = env::args().collect();
    let settings = load_settings();

    let client = Client::new();

    let mut token = load_token().unwrap_or_else(|| {
        login(&client, &settings)
    });

    let product_json = retrieve_products_json(&client, &token).unwrap_or_else(|_| {
        // If retrieving product JSON fails, the login token may have expired so try logging in again
        token = login(&client, &settings);
        retrieve_products_json(&client, &token).unwrap()
    });

    let heating_object = find_heating_object(&product_json);

    if args.len() == 1 {
        output_status(&heating_object);
    } else if args[1] == "off" {
        set_mode(&client, &heating_object, &token, "OFF");
    } else if args[1] == "manual" {
        set_mode(&client, &heating_object, &token, "MANUAL");
    } else if args[1] == "schedule" {
        set_mode(&client, &heating_object, &token, "SCHEDULE");
    } else {
        let target_temp = args[1].parse::<f64>().unwrap();
        set_target_temp(&client, &heating_object, &token, target_temp);
    }
}

fn load_settings() -> TomlValue {
    let mut path = dirs::home_dir().unwrap();
    path.push(".hheat");
    path.push("conf.toml");
    let path = path.as_path();

    let mut file = File::open(path)
        .expect("Failed to open ~/.hheat/conf.toml");
    let mut settings_string = String::new();
    file.read_to_string(&mut settings_string).unwrap();
    settings_string.parse::<TomlValue>().unwrap()
}

fn load_token() -> Option<String> {
    let mut path = dirs::home_dir().unwrap();
    path.push(".hheat");
    path.push("token");
    let path = path.as_path();

    match File::open(path) {
        Ok(mut file)  => {
	    let mut token = String::new();
            file.read_to_string(&mut token).unwrap();
            Some(token)
        }
        Err(_) => None,
    }
}

fn login(client: &Client, settings: &TomlValue) -> String {
    let token = send_login_request(&client, settings["username"].as_str().unwrap(), settings["password"].as_str().unwrap());
    // Save the login token so it can be used again, otherwise login rate limits will be hit
    save_token(&token);
    token
}

fn send_login_request(client: &Client, username: &str, password: &str) -> String {
    let resp: Value = client
        .post(&format!("{}global/login", HIVE_API_ENDPOINT))
        .body(format!("{{\"username\":\"{}\",\"password\":\"{}\",\"devices\":true,\"products\":true,\"actions\":true,\"homes\":true}}", username, password))
        .send()
        .expect("Login request failed")
        .json()
        .expect("Failed to parse login response");

    let token = resp["token"].as_str().expect(&format!("Failed to get login token: {}", resp));
    String::from(token)
}

fn save_token(token: &str) {
    let mut path = dirs::home_dir().unwrap();
    path.push(".hheat");
    path.push("token");
    let path = path.as_path();

    fs::write(path, token)
        .expect(&format!("Failed to write to {:#?}", path));
}


fn retrieve_products_json(client: &Client, token: &str) -> Result<Value, Box<dyn Error>> {
    let product_json: Value = client
        .get(&format!("{}products?after=", HIVE_API_ENDPOINT))
        .header(AUTHORIZATION, HeaderValue::from_str(token)?)
        .send()?
        .json()
        .expect("Failed to parse products response");

    let map_response = product_json.as_object();
    if map_response.is_some() && map_response.unwrap().get("error").is_some() {
        Err(Box::new(IoError::new(ErrorKind::Other, format!("Error getting JSON: {}", map_response.unwrap().get("error").unwrap()))))
    } else {
        Ok(product_json)
    }
}

fn find_heating_object(product_json: &Value) -> &Map<String, Value> {
    for product in product_json.as_array().unwrap() {
        let product_object = product.as_object().unwrap();
        if product_object["type"].as_str().unwrap() == "heating" {
            return product_object;
        }
    }

   panic!()
}

fn output_status(heating_object: &Map<String, Value>) {
    let state = heating_object["state"].as_object().unwrap();
    let props = heating_object["props"].as_object().unwrap();

    let mode = state["mode"].as_str().unwrap();
    let target_temp = state["target"].as_f64().unwrap();
    let temp = props["temperature"].as_f64().unwrap();
    let working = props["working"].as_bool().unwrap();

    let working_indicator = if working && mode != "OFF" {"🔥"} else {""};
    println!("Mode          {:>8}", mode.to_lowercase());
    println!("Temperature   {:>7.1}°", temp);
    println!("Target        {:>7.1}° {}", target_temp, working_indicator);
}

fn set_target_temp(client: &Client, heating_object: &Map<String, Value>, token: &str, target_temp: f64) {
    let state = heating_object["state"].as_object().unwrap();
    let mode = state["mode"].as_str().unwrap();

    let body = if mode == "OFF" {
        // If heating is off then switch to manual when setting temperature
        format!("{{\"target\":{}, \"mode\": MANUAL}}", target_temp)
    } else {
        format!("{{\"target\":{}}}", target_temp)
    };

    let device_id = heating_object["id"].as_str().unwrap();
    client
        .post(&format!("{}nodes/heating/{}", HIVE_API_ENDPOINT, device_id))
        .header(AUTHORIZATION, HeaderValue::from_str(token).unwrap())
        .body(body)
        .send()
        .expect("Set temperature request failed");
}

fn set_mode(client: &Client, heating_object: &Map<String, Value>, token: &str, mode: &str) {
    let device_id = heating_object["id"].as_str().unwrap();
    client
        .post(&format!("{}nodes/heating/{}", HIVE_API_ENDPOINT, device_id))
        .header(AUTHORIZATION, HeaderValue::from_str(token).unwrap())
        .body(format!("{{\"mode\":{}}}", mode))
        .send()
        .expect("Set mode request failed");
}

