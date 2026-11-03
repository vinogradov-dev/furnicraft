//! The machining pipeline: a panel's operations compile to effects on one plan.
//!
//! A [`Machining`] record says what an operation is; it never says what it does to the body. The
//! compiler reads the record's [`MachiningDescriptor`](furnicraft_model::MachiningDescriptor) and
//! turns it into [`OpEffect`]s: a region removed before extrusion, a tool body subtracted or added
//! after it, or an attachment that leaves the solid alone. [`PanelPlan`] folds those effects in list
//! order and builds the one [`SolidPlan`] the kernel evaluates.
//!
//! The descriptor table lives in the document, because the card reads it and `ui` cannot depend on
//! this crate. The compiler is the only place that turns a kind into geometry, so the panel builder
//! never matches on a kind.

use furnicraft_geom::{Frame, Region, subtract_regions};
use furnicraft_kernel::BooleanOp;
use furnicraft_model::{EdgeRef, Machining, MachiningParams, MachiningStage, Panel, PanelGeometry};
use furnicraft_units::Length;

use crate::profile;
use crate::solid::{PlanBuilder, PlanStep, SolidPlan};
use crate::{ParamError, build_cutout_tool};

/// One effect an operation has on the panel body.
///
/// Effects are derived, never stored: the document keeps the parameters and the compiler rebuilds
/// them. A shape effect changes the planar core, a post effect combines a tool body with the solid,
/// and an attachment names an edge whose part is built by the banding path.
#[derive(Clone, Debug, PartialEq)]
pub enum OpEffect {
    /// Removes a region from the planar core before the body is extruded.
    CutRegion(Region),
    /// Subtracts the tool body from the body built so far.
    Subtract(SolidPlan),
    /// Adds the tool body to the body built so far.
    Add(SolidPlan),
    /// Attaches a body along one outline edge; the panel solid does not change.
    Attachment(EdgeRef),
}

/// The compiled effects of one machining record.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledMachining {
    stage: MachiningStage,
    effects: Vec<OpEffect>,
}

impl CompiledMachining {
    fn new(stage: MachiningStage, effects: Vec<OpEffect>) -> Self {
        Self { stage, effects }
    }

    /// The stage the effects apply in, read from the kind's descriptor.
    pub const fn stage(&self) -> MachiningStage {
        self.stage
    }

    /// The effects, in the order the operation produces them.
    pub fn effects(&self) -> &[OpEffect] {
        &self.effects
    }
}

/// Compiles machining records into effects on one panel body.
///
/// The compiler is the one place a parameter kind becomes geometry. It reads the descriptor's
/// capabilities first, so a record whose anchor the kind does not accept is refused before any
/// tool is built.
pub struct CompileMachining<'a> {
    geometry: &'a PanelGeometry,
}

impl<'a> CompileMachining<'a> {
    /// Compiles against one resolved panel geometry.
    pub const fn new(geometry: &'a PanelGeometry) -> Self {
        Self { geometry }
    }

    /// The staged effects of one record.
    pub fn compile(&self, machining: &Machining) -> Result<CompiledMachining, ParamError> {
        let descriptor = machining.params().descriptor();
        if !descriptor.capabilities().accepts_anchor(machining.anchor()) {
            return Err(ParamError::MachiningAnchorUnsupported { machining: machining.id() });
        }
        let effects = match machining.params() {
            MachiningParams::Cutout(cutout) => {
                let entry = machining
                    .anchor()
                    .as_face()
                    .ok_or(ParamError::MachiningAnchorUnsupported { machining: machining.id() })?;
                vec![OpEffect::Subtract(build_cutout_tool(self.geometry, cutout, entry)?)]
            }
        };
        Ok(CompiledMachining::new(descriptor.stage(), effects))
    }
}

/// The panel body as it is folded from its machining effects.
///
/// The plan keeps the core region, the post-extrusion steps and the attachments apart, so a shape
/// effect can arrive after a post effect in list order and still be applied before the extrusion.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelPlan {
    core: Region,
    thickness: Length,
    steps: Vec<PlanStep>,
    attachments: Vec<EdgeRef>,
}

impl PanelPlan {
    /// A plan with the resolved core and no operation applied.
    pub fn new(geometry: &PanelGeometry) -> Self {
        Self {
            core: geometry.core_region().clone(),
            thickness: geometry.thickness(),
            steps: Vec::new(),
            attachments: Vec::new(),
        }
    }

    /// Folds one panel's machining records, in their list order.
    pub fn of_panel(panel: &Panel, geometry: &PanelGeometry) -> Result<Self, ParamError> {
        let compiler = CompileMachining::new(geometry);
        let mut plan = Self::new(geometry);
        for machining in panel.machining() {
            plan.fold(compiler.compile(machining)?)?;
        }
        Ok(plan)
    }

    /// Folds one compiled record into the plan.
    ///
    /// A shape effect subtracts from the core region, a post effect appends a boolean step, and an
    /// attachment is kept for the banding path. The deform stage is reserved for the deferred bend
    /// and is refused instead of being applied in the wrong order.
    pub fn fold(&mut self, compiled: CompiledMachining) -> Result<(), ParamError> {
        if compiled.stage == MachiningStage::Deform {
            return Err(ParamError::UnsupportedMachiningStage { stage: compiled.stage });
        }
        for effect in compiled.effects {
            match effect {
                OpEffect::CutRegion(region) => self.cut_region(&region)?,
                OpEffect::Subtract(tool) => self.steps.push(PlanStep::new(BooleanOp::Difference, tool)),
                OpEffect::Add(tool) => self.steps.push(PlanStep::new(BooleanOp::Union, tool)),
                OpEffect::Attachment(edge) => self.attachments.push(edge),
            }
        }
        Ok(())
    }

    /// Subtracts a shape effect from the core, keeping it one connected piece.
    fn cut_region(&mut self, region: &Region) -> Result<(), ParamError> {
        let remaining = subtract_regions(std::slice::from_ref(&self.core), std::slice::from_ref(region))?;
        let mut regions = remaining.into_iter();
        let Some(core) = regions.next() else {
            return Err(ParamError::CoreNotOneRegion);
        };
        if regions.next().is_some() {
            return Err(ParamError::CoreNotOneRegion);
        }
        self.core = core;
        Ok(())
    }

    /// The body plan built from the folded effects.
    pub fn solid(&self) -> Result<SolidPlan, ParamError> {
        let profile = profile::profile_of_region(&self.core)?;
        let mut builder = PlanBuilder::new().extrude(profile, Frame::WORLD, self.thickness.millimetres_f64())?;
        for step in &self.steps {
            builder = builder.boolean(step.op(), step.tool().clone());
        }
        Ok(builder.build()?)
    }

    /// The core region after the shape effects.
    pub const fn core_region(&self) -> &Region {
        &self.core
    }

    /// The panel thickness the body is extruded by.
    pub const fn thickness(&self) -> Length {
        self.thickness
    }

    /// The post-extrusion boolean steps in application order.
    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    /// The edges an operation attaches a part to, in list order.
    pub fn attachments(&self) -> &[EdgeRef] {
        &self.attachments
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::{DVec2, DVec3, Rect, Region};
    use furnicraft_kernel::{Profile, SolidKernel};
    use furnicraft_kernel_truck::TruckKernel;
    use furnicraft_model::{
        Container, Cutout, CutoutDepth, CutoutShape, EdgeId, FaceSide, Id, MachiningAnchor, MachiningId,
        PanelNominalSize, PanelRect, Placement, Project,
    };

    use crate::{TOOL_OVERSHOOT_MM, extrusion_plan};

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    fn panel_size() -> PanelNominalSize {
        PanelNominalSize::new(millimetres(700), millimetres(500)).expect("panel size")
    }

    fn panel() -> Panel {
        Panel::new(Id::new(1).expect("id"), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("panel")
    }

    fn panel_with_cutout(shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> Panel {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        project.add_cutout(panel_id, shape, depth, entry).expect("add cutout");
        project.panel(panel_id).expect("panel").clone()
    }

    fn geometry(panel: &Panel) -> PanelGeometry {
        panel.geometry_with_fallbacks().expect("panel geometry")
    }

    fn rectangle(x: i64, y: i64, width: i64, height: i64) -> PanelRect {
        PanelRect::new(millimetres(x), millimetres(y), millimetres(width), millimetres(height)).expect("a valid rect")
    }

    fn cutout_record(id: u64, shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> Machining {
        Machining::new(
            MachiningId::new(id).expect("machining id"),
            MachiningAnchor::face(entry),
            MachiningParams::Cutout(Cutout::new(shape, depth)),
        )
    }

    fn cut_from(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
        Region::from_rect(Rect::new(DVec2::new(x0, y0), DVec2::new(x1, y1)).expect("a valid rect"))
            .expect("a valid region")
    }

    fn volume(plan: &PanelPlan) -> f64 {
        let mut kernel = TruckKernel::new();
        let body = crate::solid::evaluate(&plan.solid().expect("solid plan"), &mut kernel).expect("body");
        kernel.measure(body).expect("measure").volume
    }

    #[test]
    fn a_cutout_compiles_to_one_post_subtraction() {
        let panel = panel();
        let record =
            cutout_record(5, CutoutShape::Rectangle(rectangle(100, 100, 100, 50)), CutoutDepth::Through, FaceSide::A);
        let compiled = CompileMachining::new(&geometry(&panel)).compile(&record).expect("compiled cutout");
        assert_eq!(compiled.stage(), MachiningStage::Post);
        assert_eq!(compiled.effects().len(), 1);
        let [OpEffect::Subtract(tool)] = compiled.effects() else {
            panic!("a cutout subtracts one tool");
        };
        assert!(tool.steps().is_empty(), "a cutout tool is one extrusion");
        assert_eq!(tool.base().frame().origin, DVec3::new(100.0, 100.0, -TOOL_OVERSHOOT_MM));
    }

    #[test]
    fn the_compiler_refuses_an_anchor_the_descriptor_does_not_accept() {
        let panel = panel();
        let record = Machining::new(
            MachiningId::new(5).expect("machining id"),
            MachiningAnchor::edge(EdgeRef::Rect { edge: EdgeId::new(1).expect("edge id") }),
            MachiningParams::Cutout(Cutout::new(
                CutoutShape::Rectangle(rectangle(100, 100, 100, 50)),
                CutoutDepth::Through,
            )),
        );
        let error = CompileMachining::new(&geometry(&panel)).compile(&record).expect_err("an edge cannot cut");
        assert!(matches!(error, ParamError::MachiningAnchorUnsupported { machining } if machining == record.id()));
    }

    #[test]
    fn a_panel_plan_folds_each_cutout_into_one_plan() {
        let panel =
            panel_with_cutout(CutoutShape::Rectangle(rectangle(100, 100, 100, 50)), CutoutDepth::Through, FaceSide::A);
        let plan = PanelPlan::of_panel(&panel, &geometry(&panel)).expect("panel plan");
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(plan.steps()[0].op(), BooleanOp::Difference);
        assert!(plan.attachments().is_empty());
        let expected = 6_300_000.0 - 100.0 * 50.0 * 18.0;
        assert!((volume(&plan) - expected).abs() < 1.0, "volume was {}", volume(&plan));
    }

    #[test]
    fn a_shape_effect_cuts_the_core_before_it_is_extruded() {
        let panel = panel();
        let mut plan = PanelPlan::new(&geometry(&panel));
        plan.fold(CompiledMachining::new(
            MachiningStage::Shape,
            vec![OpEffect::CutRegion(cut_from(-10.0, -10.0, 100.0, 100.0))],
        ))
        .expect("folded shape effect");
        assert!(plan.steps().is_empty(), "a shape effect is not a boolean step");
        let expected = (700.0 * 500.0 - 100.0 * 100.0) * 18.0;
        assert!((volume(&plan) - expected).abs() < 1.0, "volume was {}", volume(&plan));
    }

    #[test]
    fn an_add_effect_unions_its_tool_with_the_body() {
        let panel = panel();
        let mut plan = PanelPlan::new(&geometry(&panel));
        let tool = extrusion_plan(
            Profile::rectangle(10.0, 10.0).expect("tool profile"),
            Frame { origin: DVec3::new(100.0, 100.0, 17.0), ..Frame::WORLD },
            5.0,
        )
        .expect("tool plan");
        plan.fold(CompiledMachining::new(MachiningStage::Post, vec![OpEffect::Add(tool)])).expect("folded add effect");
        assert_eq!(plan.steps()[0].op(), BooleanOp::Union);
        assert!((volume(&plan) - (6_300_000.0 + 400.0)).abs() < 1.0, "volume was {}", volume(&plan));
    }

    #[test]
    fn an_attachment_leaves_the_solid_alone_and_is_kept_for_the_banding_path() {
        let panel = panel();
        let mut plan = PanelPlan::new(&geometry(&panel));
        let edge = EdgeRef::Rect { edge: EdgeId::new(1).expect("edge id") };
        plan.fold(CompiledMachining::new(MachiningStage::Shape, vec![OpEffect::Attachment(edge)]))
            .expect("folded attachment");
        assert_eq!(plan.attachments(), &[edge]);
        assert!(plan.steps().is_empty());
        assert!((volume(&plan) - 6_300_000.0).abs() < 1.0, "an attachment leaves the body alone");
    }

    #[test]
    fn a_deform_operation_is_reserved_for_the_deferred_bend() {
        let panel = panel();
        let mut plan = PanelPlan::new(&geometry(&panel));
        let error = plan
            .fold(CompiledMachining::new(MachiningStage::Deform, Vec::new()))
            .expect_err("the deform stage is not evaluated");
        assert!(matches!(error, ParamError::UnsupportedMachiningStage { stage: MachiningStage::Deform }));
    }

    #[test]
    fn a_shape_effect_that_splits_the_core_is_refused() {
        let panel = panel();
        let mut plan = PanelPlan::new(&geometry(&panel));
        let error = plan
            .fold(CompiledMachining::new(
                MachiningStage::Shape,
                vec![OpEffect::CutRegion(cut_from(-10.0, 245.0, 710.0, 255.0))],
            ))
            .expect_err("a split core is not a panel");
        assert!(matches!(error, ParamError::CoreNotOneRegion));
    }

    #[test]
    fn a_shape_effect_that_removes_the_core_is_refused() {
        let panel = panel();
        let mut plan = PanelPlan::new(&geometry(&panel));
        let error = plan
            .fold(CompiledMachining::new(
                MachiningStage::Shape,
                vec![OpEffect::CutRegion(cut_from(-10.0, -10.0, 710.0, 510.0))],
            ))
            .expect_err("an empty core is not a panel");
        assert!(matches!(error, ParamError::CoreNotOneRegion));
    }
}
