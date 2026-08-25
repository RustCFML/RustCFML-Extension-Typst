//! Thin adapter: hand the engine the module's registrations.
use rustcfml_cli::Vm;

pub fn register(vm: &mut Vm) {
    rustcfml_typst::register(vm);
}
