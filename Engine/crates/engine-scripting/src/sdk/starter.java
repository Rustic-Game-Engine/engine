class RusticBehavior extends Rustic {
    public static void main(String[] args) throws Exception {
        run((callback, dt) -> {
            if (callback.equals("on_start")) rustic.log("info", "Behavior started");
            if (callback.equals("fixed_update") && rustic.key("KeyW").held) {
                double[] p = rustic.get_translation();
                rustic.set_translation(p[0], p[1], p[2] + dt);
            }
        });
    }
}
