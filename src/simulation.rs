use crate::protocol::{InputCommand, MoveInput, Vec3};

pub(crate) fn sanitize_input(input: InputCommand) -> InputCommand {
    InputCommand {
        movement: clamp_move(input.movement),
        ..input
    }
}

pub(crate) fn movement_velocity(input: &InputCommand, move_speed: f32) -> Vec3 {
    let forward_x = -input.yaw.sin();
    let forward_z = -input.yaw.cos();
    let right_x = -forward_z;
    let right_z = forward_x;

    Vec3 {
        x: (right_x * input.movement.x + forward_x * input.movement.z) * move_speed,
        y: 0.0,
        z: (right_z * input.movement.x + forward_z * input.movement.z) * move_speed,
    }
}

fn clamp_move(m: MoveInput) -> MoveInput {
    let x = m.x.clamp(-1.0, 1.0);
    let z = m.z.clamp(-1.0, 1.0);
    let len = (x * x + z * z).sqrt();
    if len > 1.0 {
        MoveInput {
            x: x / len,
            z: z / len,
        }
    } else {
        MoveInput { x, z }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{InputCommand, MoveInput};

    #[test]
    fn clamps_diagonal_move_to_unit_length() {
        let input = sanitize_input(InputCommand {
            seq: 1,
            client_tick: 1,
            dt_ms: 16,
            movement: MoveInput { x: 1.0, z: 1.0 },
            yaw: 0.0,
            pitch: 0.0,
        });

        let len =
            (input.movement.x * input.movement.x + input.movement.z * input.movement.z).sqrt();
        assert!(len <= 1.0001);
    }
}
