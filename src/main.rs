use rusoto_core::Region;
use rusoto_ec2::{Ec2, Ec2Client, DescribeSpotPriceHistoryRequest, DescribeInstanceTypesRequest};
use clap::{Arg, Command};
use chrono::{Utc, Duration};
use colored::*;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    let custom_art = r#"
    _____  _       _  ___       ___                  _    _                
   (  _  )( )  _  ( )(  _`\    (  _`\               ( )_ ( )_              
   | (_) || | ( ) | || (_(_)   | (_(_) _ _      _   | ,_)| ,_)   __   _ __ 
   |  _  || | | | | |`\__ \    `\__ \ ( '_`\  /'_`\ | |  | |   /'__`\( '__)
   | | | || (_/ \_) |( )_) |   ( )_) || (_) )( (_) )| |_ | |_ (  ___/| |   
   (_) (_)`\___x___/'`\____)   `\____)| ,__/'`\___/'`\__)`\__)`\____)(_)   
                                      | |                                  
                                      (_)                                  
   "#;
    println!("{}", custom_art.cyan());

    // Parse command-line arguments using clap
    let matches = Command::new("AWS Spot Price Checker")
        .version("3.2.1")
        .author("Daniel James <mail@danjames.co.uk>")
        .about("Check AWS Spot Instance prices")
        .arg(
            Arg::new("instance_types")
                .help("EC2 instance types")
                .required(true)
                .num_args(1..)  // Accept multiple values for instance types
                .index(1),
        )
        .arg(
            Arg::new("region")
                .short('r')
                .long("region")
                .help("Specify the AWS region")
                .value_name("REGION"),
        )
        .get_matches();

    let ec2_types: Vec<_> = matches
        .get_many::<String>("instance_types")
        .unwrap()
        .map(|s| s.as_str())
        .collect();
    let region = matches
        .get_one::<String>("region")
        .map_or("us-west-2", |s| s.as_str());

    // Iterate over each instance type and get spot prices and architecture
    for ec2_type in ec2_types {
        println!("{}\n======================\n", "AWS Spot Price Checker".bright_blue());
        println!("Instance Type: {}", ec2_type.bright_green());
        println!("Region: {}\n", region.bright_green());

        match get_instance_details(ec2_type, region).await {
            Ok((architecture, vcpus, memory)) => {
                println!("Architecture: {}", architecture.bright_green());
                println!("vCPU's: {}", vcpus.to_string().bright_green());
                println!("Memory: {} GiB\n", memory.to_string().bright_green());
            },
            Err(e) => eprintln!("Error fetching instance details for {}: {}", ec2_type, e),
        }

        if let Err(e) = get_spot_prices(ec2_type, region).await {
            eprintln!("Error fetching spot prices for {}: {}", ec2_type, e);
        }

        println!("\n");
    }
}

async fn get_spot_prices(instance_type: &str, region: &str) -> Result<(), Box<dyn std::error::Error>> {
    let region = region.parse::<Region>()?;
    let ec2 = Ec2Client::new(region);

    let start_time = Utc::now() - Duration::hours(4);
    let request = DescribeSpotPriceHistoryRequest {
        instance_types: Some(vec![instance_type.to_string()]),
        product_descriptions: Some(vec![
            "Linux/UNIX".to_string(),
            "Linux/UNIX (Amazon VPC)".to_string(),
        ]),
        start_time: Some(start_time.to_rfc3339()),
        ..Default::default()
    };

    let result = ec2.describe_spot_price_history(request).await?;
    let mut zones: HashMap<String, (String, f64)> = HashMap::new();
    let mut low_price = f64::MAX;
    let mut low_zone = String::new();

    if let Some(spot_prices) = result.spot_price_history {
        for spot in spot_prices {
            if let (Some(price), Some(zone), Some(timestamp)) = (
                spot.spot_price,
                spot.availability_zone,
                spot.timestamp,
            ) {
                let price: f64 = price.parse()?;
                let timestamp = timestamp.to_string();

                zones
                    .entry(zone.clone())
                    .and_modify(|(prev_ts, prev_price)| {
                        if &timestamp > prev_ts {
                            *prev_ts = timestamp.clone();
                            *prev_price = price;
                        }
                    })
                    .or_insert((timestamp, price));

                if price < low_price {
                    low_price = price;
                    low_zone = zone.clone();
                }
            }
        }
    }

    println!(
        "{}",
        "\n-----------------------------------------------
| Availability Zone      | Hourly Rate         |
-----------------------------------------------"
            .bold()
    );

    let mut sorted_zones: Vec<_> = zones.iter().collect();
    sorted_zones.sort_by(|a, b| a.1 .1.partial_cmp(&b.1 .1).unwrap());

    for (zone, (_, price)) in sorted_zones {
        let msg = format!("| {:<22} | ${:<18} |", zone, price);
        if zone == &low_zone {
            println!("{}", msg.green());
        } else if low_price * 1.1 >= *price {
            println!("{}", msg.yellow());
        } else {
            println!("{}", msg);
        }
    }

    println!("-----------------------------------------------");

    println!(
        "\n💡 Cheapest hourly rate: ${} in zone {}",
        low_price.to_string().bright_green(),
        low_zone.bright_green()
    );

    Ok(())
}

async fn get_instance_details(instance_type: &str, region: &str) -> Result<(String, i64, f64), Box<dyn std::error::Error>> {
    let region = region.parse::<Region>()?;
    let ec2 = Ec2Client::new(region);

    let request = DescribeInstanceTypesRequest {
        instance_types: Some(vec![instance_type.to_string()]),
        ..Default::default()
    };

    let result = ec2.describe_instance_types(request).await?;

    if let Some(instance_types) = result.instance_types {
        if let Some(instance) = instance_types.into_iter().next() {
            let architecture = instance.processor_info
                .and_then(|p| p.supported_architectures)
                .unwrap_or_default()
                .join(", ");
            let vcpus = instance.v_cpu_info.map_or(0, |v| v.default_v_cpus.unwrap_or(0));
            let memory = instance.memory_info
                .map_or(0.0, |m| m.size_in_mi_b.unwrap_or(0) as f64 / 1024.0); // Convert MiB to GiB

            return Ok((architecture, vcpus, memory));
        }
    }

    Err("Instance details not found".into())
}
