use crate::{error::DomainError, value_objects::non_blank};

const MAX_NAME_LEN: usize = 100;

/// Who receives the parcel and how the drone operator can reach them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    name: String,
    phone: String,
}

impl Recipient {
    pub fn new(name: impl AsRef<str>, phone: impl AsRef<str>) -> Result<Self, DomainError> {
        let name = non_blank("recipient name", name, MAX_NAME_LEN)?;
        let phone = phone.as_ref().trim();

        // E.164: '+' followed by 8-15 digits.
        let valid = phone.strip_prefix('+').is_some_and(|digits| {
            (8..=15).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit())
        });
        if !valid {
            return Err(DomainError::InvalidPhoneNumber);
        }

        Ok(Self {
            name,
            phone: phone.to_owned(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn phone(&self) -> &str {
        &self.phone
    }
}
