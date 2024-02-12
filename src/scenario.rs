use crate::global;
use crate::global::Global;
use crate::options;
use crate::options::Options;
use diameter::avp::Address;
use diameter::avp::AvpType;
use diameter::avp::AvpValue;
use diameter::avp::Enumerated;
use diameter::avp::Identity;
use diameter::avp::OctetString;
use diameter::avp::UTF8String;
use diameter::avp::Unsigned32;
use diameter::avp::Unsigned64;
use diameter::dictionary;
use diameter::flags;
use diameter::{ApplicationId, CommandCode, DiameterMessage};
use regex::Regex;
use std::error::Error;
use std::result::Result;

pub struct Scenario<'a> {
    message: Message<'a>,
}

impl<'a> Scenario<'a> {
    pub fn new(options: &Options, global: &'a Global) -> Result<Self, Box<dyn Error>> {
        return Ok(Scenario {
            message: Message::new(&options.scenarios.get(1).unwrap(), global)?,
        });
    }

    pub fn next_message(&mut self) -> Result<DiameterMessage, Box<dyn Error>> {
        self.message.message()
    }
}

pub struct Message<'a> {
    command_code: CommandCode,
    application_id: ApplicationId,
    flags: u8,
    seq_num: u32,
    avps: Vec<Avp<'a>>,
}

impl<'a> Message<'a> {
    pub fn new(scenario: &options::Scenario, global: &'a Global) -> Result<Self, Box<dyn Error>> {
        // TODO remove hardcode, get command_code and app_id from dictionary
        // TODO!
        let command_code = CommandCode::CreditControl;
        let application_id = ApplicationId::CreditControl;
        let flags = flags::REQUEST;

        let mut avps = vec![];

        for a in &scenario.message.avps {
            let avp_definition = dictionary::DEFAULT_DICT
                .get_avp_by_name(&a.name)
                .ok_or("AVP not found in dictionary")?;

            let value = Value::new(&a.value, avp_definition.avp_type, global);

            let avp = Avp {
                code: avp_definition.code,
                vendor_id: None, // TODO remove hardcode, avp_definition.vendor_id,
                flags: 0,        // TODO remove hardcode, avp_definition.flags,
                value,
            };

            avps.push(avp);
        }

        Ok(Message {
            command_code,
            application_id,
            flags,
            seq_num: 0,
            avps,
        })
    }

    pub fn message(&mut self) -> Result<DiameterMessage, Box<dyn Error>> {
        self.seq_num += 1;
        let mut diameter_msg = DiameterMessage::new(
            self.command_code,
            self.application_id,
            self.flags,
            self.seq_num,
            self.seq_num,
        );

        for avp in &self.avps {
            let value = avp.value.get_value()?;
            diameter_msg.add_avp(diameter::avp::Avp::new(
                avp.code,
                avp.vendor_id,
                avp.flags,
                value,
            ));
        }

        Ok(diameter_msg)
    }
}

pub fn string_to_avp_value(
    str: &str,
    avp_type: diameter::avp::AvpType,
) -> Result<AvpValue, Box<dyn Error>> {
    let value = match avp_type {
        AvpType::Address => Address::new(str.as_bytes().to_vec()).into(),
        AvpType::AddressIPv4 => Address::new(str.as_bytes().to_vec()).into(),
        AvpType::AddressIPv6 => Address::new(str.as_bytes().to_vec()).into(),
        AvpType::Identity => Identity::new(&str).into(),
        AvpType::DiameterURI => UTF8String::new(&str).into(),
        AvpType::Enumerated => Enumerated::new(str.parse().unwrap()).into(),
        AvpType::Float32 => Unsigned32::new(str.parse().unwrap()).into(),
        AvpType::Float64 => Unsigned64::new(str.parse().unwrap()).into(),
        AvpType::Grouped => todo!(),
        AvpType::Integer32 => Unsigned32::new(str.parse().unwrap()).into(),
        AvpType::Integer64 => Unsigned64::new(str.parse().unwrap()).into(),
        AvpType::OctetString => OctetString::new(str.as_bytes().to_vec()).into(),
        AvpType::Unsigned32 => Unsigned32::new(str.parse().unwrap()).into(),
        AvpType::Unsigned64 => Unsigned64::new(str.parse().unwrap()).into(),
        AvpType::UTF8String => UTF8String::new(&str).into(),
        AvpType::Time => Unsigned32::new(str.parse().unwrap()).into(),
        AvpType::Unknown => return Err("Unknown AVP type".into()),
    };
    Ok(value)
}

struct Avp<'a> {
    code: u32,
    vendor_id: Option<u32>,
    flags: u8,
    value: Value<'a>,
}

struct Value<'a> {
    source: String,
    avp_type: diameter::avp::AvpType,
    variables: Vec<&'a global::Variable>,
    constant: Option<AvpValue>,
}

impl<'a> Value<'a> {
    pub fn new(source: &str, avp_type: diameter::avp::AvpType, global: &'a Global) -> Self {
        // Scan for variables
        let variable_pattern = Regex::new(r"\$\{([^}]+)\}").unwrap();
        let mut variables = vec![];
        for caps in variable_pattern.captures_iter(source) {
            let cap = caps[1].to_string();
            let var = global.get_variable(&cap).unwrap();
            variables.push(var);
        }

        // If no variable found, make this a constant
        let constant = if variables.is_empty() {
            let value = string_to_avp_value(source, avp_type).unwrap();
            Some(value)
        } else {
            None
        };

        Value {
            source: source.into(),
            avp_type,
            variables,
            constant,
        }
    }

    fn compute(&self) -> String {
        let mut result: String = self.source.clone();
        for v in &self.variables {
            let counter = v.value.get();
            let name = &v.name;
            result = result.replace(&format!("${{{}}}", name), &counter);
        }
        result
    }

    pub fn get_value(&self) -> Result<AvpValue, Box<dyn Error>> {
        match &self.constant {
            Some(v) => Ok(v.clone()),
            None => Ok(string_to_avp_value(&self.compute(), self.avp_type)?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options;

    #[test]
    fn test_constant() {
        let global = Global::new(&options::Global {
            variables: vec![std::iter::once((
                "COUNTER".into(),
                options::Variable {
                    func: options::Function::IncrementalCounter,
                    min: 1,
                    max: 5,
                    step: 3,
                },
            ))
            .collect()],
        });

        let variable = Value::new("example.origin.host", AvpType::UTF8String, &global);

        assert_eq!("example.origin.host", variable.compute());
        assert_eq!("example.origin.host", variable.compute());
        assert_eq!("example.origin.host", variable.compute());
    }

    #[test]
    fn test_counter_variable() {
        let global = Global::new(&options::Global {
            variables: vec![std::iter::once((
                "COUNTER".into(),
                options::Variable {
                    func: options::Function::IncrementalCounter,
                    min: 1,
                    max: 5,
                    step: 3,
                },
            ))
            .collect()],
        });

        let variable = Value::new("ses;${COUNTER}", AvpType::UTF8String, &global);

        assert_eq!("ses;1", variable.compute());
        assert_eq!("ses;4", variable.compute());
        assert_eq!("ses;1", variable.compute());
    }

    #[test]
    fn test_2_counters_variable() {
        let global = Global::new(&options::Global {
            variables: vec![
                std::iter::once((
                    "COUNTER1".into(),
                    options::Variable {
                        func: options::Function::IncrementalCounter,
                        min: 0,
                        max: 5,
                        step: 1,
                    },
                ))
                .collect(),
                std::iter::once((
                    "COUNTER2".into(),
                    options::Variable {
                        func: options::Function::IncrementalCounter,
                        min: 1,
                        max: 5,
                        step: 3,
                    },
                ))
                .collect(),
            ],
        });

        let variable = Value::new("ses;${COUNTER1}_${COUNTER2}", AvpType::UTF8String, &global);

        assert_eq!("ses;0_1", variable.compute());
        assert_eq!("ses;1_4", variable.compute());
        assert_eq!("ses;2_1", variable.compute());
    }
}