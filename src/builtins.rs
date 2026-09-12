use crate::ast::{ConstantValue, ReturnType, ValueType};

pub struct BuiltinFunction {
    pub name: &'static str,
    pub params: &'static [ValueType],
    pub return_type: ReturnType,
    pub llvm_symbol: &'static str,
}

#[derive(Clone)]
pub struct PredefinedConstant {
    pub name: &'static str,
    pub value: ConstantValue,
}

pub const FUNCTIONS: &[BuiltinFunction] = &[
    BuiltinFunction {
        name: "tone",
        params: &[ValueType::F32, ValueType::F32, ValueType::F32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_tone",
    },
    BuiltinFunction {
        name: "noise",
        params: &[ValueType::F32, ValueType::F32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_noise",
    },
    BuiltinFunction {
        name: "sin",
        params: &[ValueType::F32],
        return_type: ReturnType::Value(ValueType::F32),
        llvm_symbol: "@crumb_sin",
    },
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
        params: &[const { ValueType::I32 }; 7],
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

const KEY_CONSTANTS: &[PredefinedConstant] = &[
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
    PredefinedConstant {
        name: "KEY_F",
        value: ConstantValue::I32(11),
    },
];

/// All predefined values for one game's logical framebuffer.
pub fn constants(
    resolution: crate::resolution::Resolution,
) -> impl Iterator<Item = PredefinedConstant> {
    KEY_CONSTANTS.iter().cloned().chain([
        PredefinedConstant {
            name: "FRAMEBUFFER_WIDTH",
            value: ConstantValue::I32(i32::from(resolution.width())),
        },
        PredefinedConstant {
            name: "FRAMEBUFFER_HEIGHT",
            value: ConstantValue::I32(i32::from(resolution.height())),
        },
    ])
}

pub fn is_predefined_constant(name: &str) -> bool {
    constants(crate::resolution::Resolution::DEFAULT).any(|constant| constant.name == name)
}
