use crate::ast::{ConstantValue, ReturnType, ValueType};

pub struct BuiltinFunction {
    pub name: &'static str,
    pub params: &'static [ValueType],
    pub return_type: ReturnType,
    pub llvm_symbol: &'static str,
}

pub struct PredefinedConstant {
    pub name: &'static str,
    pub value: ConstantValue,
}

pub const FUNCTIONS: &[BuiltinFunction] = &[
    BuiltinFunction {
        name: "print_i32",
        params: &[ValueType::I32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_print_i32",
    },
    BuiltinFunction {
        name: "debug_frame",
        params: &[ValueType::I32, ValueType::F32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_debug_frame",
    },
    BuiltinFunction {
        name: "clear_rgb",
        params: &[ValueType::I32, ValueType::I32, ValueType::I32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_clear_rgb",
    },
    BuiltinFunction {
        name: "fill_rect",
        params: &[ValueType::I32; 7],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_fill_rect",
    },
    BuiltinFunction {
        name: "key_down",
        params: &[ValueType::I32],
        return_type: ReturnType::Value(ValueType::Bool),
        llvm_symbol: "@crumb_key_down",
    },
    BuiltinFunction {
        name: "key_pressed",
        params: &[ValueType::I32],
        return_type: ReturnType::Value(ValueType::Bool),
        llvm_symbol: "@crumb_key_pressed",
    },
    BuiltinFunction {
        name: "key_released",
        params: &[ValueType::I32],
        return_type: ReturnType::Value(ValueType::Bool),
        llvm_symbol: "@crumb_key_released",
    },
    BuiltinFunction {
        name: "quit",
        params: &[],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_request_quit",
    },
];

pub const CONSTANTS: &[PredefinedConstant] = &[
    PredefinedConstant {
        name: "KEY_W",
        value: ConstantValue::I32(0),
    },
    PredefinedConstant {
        name: "KEY_A",
        value: ConstantValue::I32(1),
    },
    PredefinedConstant {
        name: "KEY_S",
        value: ConstantValue::I32(2),
    },
    PredefinedConstant {
        name: "KEY_D",
        value: ConstantValue::I32(3),
    },
    PredefinedConstant {
        name: "KEY_UP",
        value: ConstantValue::I32(4),
    },
    PredefinedConstant {
        name: "KEY_DOWN",
        value: ConstantValue::I32(5),
    },
    PredefinedConstant {
        name: "KEY_LEFT",
        value: ConstantValue::I32(6),
    },
    PredefinedConstant {
        name: "KEY_RIGHT",
        value: ConstantValue::I32(7),
    },
    PredefinedConstant {
        name: "KEY_SPACE",
        value: ConstantValue::I32(8),
    },
    PredefinedConstant {
        name: "KEY_ENTER",
        value: ConstantValue::I32(9),
    },
    PredefinedConstant {
        name: "KEY_ESCAPE",
        value: ConstantValue::I32(10),
    },
];

pub fn predefined_constant(name: &str) -> Option<ConstantValue> {
    CONSTANTS
        .iter()
        .find(|constant| constant.name == name)
        .map(|constant| constant.value)
}
