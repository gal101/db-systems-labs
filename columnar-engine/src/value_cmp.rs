use crate::{RowID, Value};
use std::cmp::Ordering;

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self {
            Value::UInt(num) => {
                let Value::UInt(other_num) = other else { panic!() };
                num.partial_cmp(other_num)
            },
            Value::Int(num) => {
                let Value::Int(other_num) = other else { panic!() };
                num.partial_cmp(other_num)
            },
            Value::RowID(RowID(num)) => {
                let Value::RowID(RowID(other_num)) = other else { panic!() };
                num.partial_cmp(other_num)
            },
            Value::Varchar(st) => {
                let Value::Varchar(other_st) = other else { panic!() };
                st.partial_cmp(other_st)
            }
        }
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match self {
            Value::UInt(num) => {
                let Value::UInt(other_num) = other else { panic!() };
                num.cmp(other_num)
            },
            Value::Int(num) => {
                let Value::Int(other_num) = other else { panic!() };
                num.cmp(other_num)
            },
            Value::RowID(RowID(num)) => {
                let Value::RowID(RowID(other_num)) = other else { panic!() };
                num.cmp(other_num)
            },
            Value::Varchar(st) => {
                let Value::Varchar(other_st) = other else { panic!() };
                st.cmp(other_st)
            }
        }
    }
}
