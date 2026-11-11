//! The table: every user-facing string, one row per message.
//!
//! A row is `Variant = [English, Russian]`. The variant is the key, the pair is the translation, and
//! the array length is [`Lang::COUNT`], so adding a language breaks every row until it is filled in.
//! Rows are grouped by the surface that draws them; the group comment is a reading aid, not data.

use crate::Lang;

messages! {
    // ── The table's own tests ───────────────────────────────────────────────
    TestPair = ["{} and {}", "{} и {}"],
    TestPointOne = ["{} point", "{} точка"],
    TestPointFew = ["{} points", "{} точки"],
    TestPointMany = ["{} points", "{} точек"],

    // ── Commands (furnicraft-engine) ────────────────────────────────────────
    CmdCreateCamera = ["Create camera", "Создать камеру"],
    CmdCreateLight = ["Create light", "Создать источник света"],
    CmdSetActiveCamera = ["Set active camera", "Сделать камеру активной"],
    CmdSetCameraProjection = ["Set camera projection", "Задать проекцию камеры"],
    CmdSetLightParameters = ["Set light parameters", "Задать параметры света"],
    CmdCreateAssembly = ["Create assembly", "Создать сборку"],
    CmdRename = ["Rename", "Переименовать"],
    CmdDelete = ["Delete", "Удалить"],
    CmdDuplicate = ["Duplicate", "Дублировать"],
    CmdGroup = ["Group", "Сгруппировать"],
    CmdUngroup = ["Ungroup", "Разгруппировать"],
    CmdPlace = ["Place", "Разместить"],
    CmdZeroTranslation = ["Zero translation", "Обнулить перенос"],
    CmdPlaceSelection = ["Place selection", "Разместить выделение"],
    CmdSetPivot = ["Set pivot", "Задать пивот"],
    CmdCenterPivot = ["Center pivot", "Центрировать пивот"],
    CmdSetProperty = ["Set property", "Задать свойство"],
    CmdSetProperties = ["Set properties", "Задать свойства"],
    CmdClearProperty = ["Clear property", "Очистить свойство"],
    CmdConvertToGroup = ["Convert to group", "Преобразовать в группу"],
    CmdMoveVertices = ["Move vertices", "Переместить вершины"],
    CmdCreatePrimitive = ["Create primitive", "Создать примитив"],
    CmdReplacePrimitive = ["Replace primitive parameters", "Заменить параметры примитива"],
    CmdCreateExtrusion = ["Create extrusion", "Создать выдавливание"],
    CmdReplaceContour = ["Replace contour", "Заменить контур"],
    CmdExtrudePath = ["Extrude path", "Выдавить путь"],
    CmdSetExtrusionHeight = ["Set extrusion height", "Задать высоту выдавливания"],
    CmdCreatePath = ["Create path", "Создать путь"],
    CmdReplacePath = ["Replace path", "Заменить путь"],
    CmdCollapseToMesh = ["Collapse to mesh", "Свернуть в сетку"],
    CmdSetEdgeBanding = ["Set edge banding", "Задать кромку"],
    CmdClearEdgeBanding = ["Clear edge banding", "Убрать кромку"],
    CmdSetCornerBanding = ["Set corner banding", "Задать кромку угла"],
    CmdClearCornerBanding = ["Clear corner banding", "Убрать кромку угла"],
    CmdSetContourBanding = ["Set contour banding", "Задать кромку контура"],
    CmdClearContourBanding = ["Clear contour banding", "Убрать кромку контура"],
    CmdSetCornerFeature = ["Set corner feature", "Задать обработку угла"],
    CmdClearCornerFeature = ["Clear corner feature", "Убрать обработку угла"],
    CmdSetEdgeBow = ["Set edge bow", "Задать прогиб кромки"],
    CmdClearEdgeBow = ["Clear edge bow", "Убрать прогиб кромки"],
    CmdAddMachining = ["Add machining", "Добавить обработку"],
    CmdUpdateMachining = ["Update machining", "Изменить обработку"],
    CmdRemoveMachining = ["Remove machining", "Удалить обработку"],
    CmdMoveMachining = ["Move machining", "Переместить обработку"],
    CmdCreatePanel = ["Create panel", "Создать панель"],
    CmdCreateContourPanel = ["Create contour panel", "Создать панель по контуру"],
    CmdSetPanelOutline = ["Set panel outline", "Задать контур панели"],
    CmdBakePanelOutline = ["Bake panel outline into a contour", "Преобразовать контур панели в кривую"],
    CmdRenamePanel = ["Rename panel", "Переименовать панель"],
    CmdResizePanel = ["Resize panel", "Изменить размер панели"],
    CmdSetPanelMaterial = ["Set panel material", "Задать материал панели"],
}

impl Message {
    /// "1 point / 2 points / 5 points", a counted message the tests exercise.
    pub const fn test_points(n: usize) -> crate::Counted {
        Self::counted(n, Self::TestPointOne, Self::TestPointFew, Self::TestPointMany)
    }
}
