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
        name: "load_i32",
        params: &[ValueType::I32, ValueType::I32],
        return_type: ReturnType::Value(ValueType::I32),
        llvm_symbol: "@crumb_load_i32",
    },
    BuiltinFunction {
        name: "save_i32",
        params: &[ValueType::I32, ValueType::I32],
        return_type: ReturnType::Value(ValueType::Bool),
        llvm_symbol: "@crumb_save_i32",
    },
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
        name: "sound_play",
        params: &[ValueType::I32, ValueType::F32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_sound_play",
    },
    BuiltinFunction {
        name: "sound_pause",
        params: &[],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_sound_pause",
    },
    BuiltinFunction {
        name: "sound_resume",
        params: &[],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_sound_resume",
    },
    BuiltinFunction {
        name: "sound_stop",
        params: &[],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_sound_stop",
    },
    BuiltinFunction {
        name: "sound_position",
        params: &[],
        return_type: ReturnType::Value(ValueType::F32),
        llvm_symbol: "@crumb_sound_position",
    },
    BuiltinFunction {
        name: "sound_seek",
        params: &[ValueType::F32],
        return_type: ReturnType::Void,
        llvm_symbol: "@crumb_sound_seek",
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

/// All predefined values for one game's logical framebuffer.
pub fn constants(
    resolution: crate::resolution::Resolution,
) -> impl Iterator<Item = PredefinedConstant> {
    crate::keyboard::KEYS
        .iter()
        .map(|info| PredefinedConstant {
            name: info.name,
            value: ConstantValue::I32(info.key as i32),
        })
        .chain([
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
