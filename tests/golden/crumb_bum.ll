; Speck game: Crumb Bum
source_filename = "speck"

declare void @crumb_print_i32(i32)
declare void @crumb_debug_frame(i32, float)

@spk_global_x = internal global float 1.00000000e+01
@spk_global_velocity = internal global float 1.80000000e+03
@spk_global_frames = internal global i32 0
@spk_global_active = internal global i1 true

define float @spk_fn_advance(float %arg0, float %arg1, float %arg2) {
entry:
  %t0 = alloca float
  store float %arg0, ptr %t0
  %t1 = alloca float
  store float %arg1, ptr %t1
  %t2 = alloca float
  store float %arg2, ptr %t2
  %t3 = load float, ptr %t0
  %t4 = load float, ptr %t1
  %t5 = load float, ptr %t2
  %t6 = fmul float %t4, %t5
  %t7 = fadd float %t3, %t6
  ret float %t7
}

define void @spk_start() {
entry:
  call void @crumb_print_i32(i32 1440)
  %t0 = alloca i32
  store i32 2, ptr %t0
  br label %while_condition_0
while_condition_0:
  %t1 = load i32, ptr %t0
  %t2 = icmp sgt i32 %t1, 0
  br i1 %t2, label %while_body_1, label %while_end_2
while_body_1:
  %t3 = load i32, ptr %t0
  call void @crumb_print_i32(i32 %t3)
  %t4 = load i32, ptr %t0
  %t5 = sub i32 %t4, 1
  store i32 %t5, ptr %t0
  br label %while_condition_0
while_end_2:
  ret void
}

define void @spk_update(float %arg0) {
entry:
  %t0 = alloca float
  store float %arg0, ptr %t0
  %t1 = load i1, ptr @spk_global_active
  br i1 %t1, label %if_then_0, label %if_else_2
if_then_0:
  %t2 = load float, ptr @spk_global_x
  %t3 = load float, ptr @spk_global_velocity
  %t4 = load float, ptr %t0
  %t5 = call float @spk_fn_advance(float %t2, float %t3, float %t4)
  store float %t5, ptr @spk_global_x
  br label %if_end_1
if_else_2:
  store float 0.00000000e+00, ptr @spk_global_x
  br label %if_end_1
if_end_1:
  %t6 = load i32, ptr @spk_global_frames
  %t7 = add i32 %t6, 1
  store i32 %t7, ptr @spk_global_frames
  %t8 = load float, ptr @spk_global_x
  %t9 = fcmp ogt float %t8, 1.00000000e+02
  br i1 %t9, label %if_then_3, label %if_end_4
if_then_3:
  %t10 = load float, ptr @spk_global_velocity
  %t11 = fsub float 0.00000000e+00, %t10
  store float %t11, ptr @spk_global_velocity
  br label %if_end_4
if_end_4:
  %t12 = load float, ptr @spk_global_x
  %t13 = fcmp olt float %t12, 0.00000000e+00
  br i1 %t13, label %if_then_5, label %if_end_6
if_then_5:
  %t14 = load float, ptr @spk_global_velocity
  %t15 = fsub float 0.00000000e+00, %t14
  store float %t15, ptr @spk_global_velocity
  br label %if_end_6
if_end_6:
  ret void
}

define void @spk_draw() {
entry:
  %t0 = load i32, ptr @spk_global_frames
  %t1 = load float, ptr @spk_global_x
  call void @crumb_debug_frame(i32 %t0, float %t1)
  ret void
}
