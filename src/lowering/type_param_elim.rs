//! Convert type parameters into local typedefs. This will break HierScope's `names` and `symbols`
//! indices.

use std::rc::Rc;

use elaborate::ty::Ty;
use elaborate::expr::Val;
use elaborate::hier::{self, HierItem, TypedefDecl};

pub fn type_param_elim(source: &mut hier::Source) {
    let mut elim = TypeParamEliminator {};
    elim.visit(source)
}

struct TypeParamEliminator {}

impl TypeParamEliminator {
    pub fn visit_item(&mut self, item: &mut HierItem) {
        match item {
            HierItem::Design(decl) => {
                for (_, inst) in decl.instances.borrow_mut().iter_mut() {
                    self.visit_instantiation(Rc::get_mut(inst).unwrap());
                }
            }
            HierItem::GenBlock(genblk) => {
                for item in &mut Rc::get_mut(genblk).unwrap().scope.items {
                    self.visit_item(item);
                }
            }
            HierItem::LoopGenBlock(loopgenblk) => {
                for (_, genblk) in loopgenblk.instances.borrow_mut().iter_mut() {
                    for item in &mut Rc::get_mut(genblk).unwrap().scope.items {
                        self.visit_item(item);
                    }
                }
            }
            _ => (),
        }
    }

    pub fn visit_instantiation(&mut self, inst: &mut hier::DesignInstantiation) {
        ::util::replace_with(&mut inst.scope.items, |items| {
            let mut params = Vec::new();
            let mut type_params = Vec::new();
            let mut ports = Vec::new();
            let mut others = Vec::new();

            for item in items {
                match item {
                    HierItem::Param(decl) => {
                        if let Ty::Type = decl.ty {
                            type_params.push(HierItem::Type(Rc::new(TypedefDecl {
                                ty: if let Val::Type(ref ty) = decl.init { ty.clone() } else { unreachable!() },
                                name: decl.name.clone()
                            })))
                        } else {
                            params.push(HierItem::Param(decl))
                        }
                    }
                    HierItem::DataPort(decl) => ports.push(HierItem::DataPort(decl)),
                    HierItem::InterfacePort(decl) => ports.push(HierItem::InterfacePort(decl)),
                    mut item => {
                        self.visit_item(&mut item);
                        others.push(item)
                    }
                }
            }

            params.extend(ports);
            params.extend(type_params);
            params.extend(others);
            params
        })
    }

    pub fn visit(&mut self, source: &mut hier::Source) {
        for unit in &mut source.units {
            for item in &mut unit.items {
                self.visit_item(item);
            }
        }
    }
}
