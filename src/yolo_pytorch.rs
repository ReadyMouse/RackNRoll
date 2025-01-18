use pyo3::prelude::*;
use pyo3::types::IntoPyDict;
use std::path::Path;

pub struct YOLOModel {
    model: PyObject,
    py_interpreter: Python<'static>,
}

impl YOLOModel {
    pub fn new(weights_path: &Path) -> PyResult<Self> {
        let py = unsafe { Python::assume_gil_acquired() };
        
        // Import required Python modules
        let ultralytics = py.import("ultralytics")?;
        let torch = py.import("torch")?;
        
        // Load the YOLO model with custom weights
        let model = ultralytics.getattr("YOLO")?.call1((weights_path.to_str().unwrap(),))?;
        
        Ok(YOLOModel {
            model: model.into(),
            py_interpreter: py,
        })
    }
    
    pub fn predict(&self, image_path: &Path) -> PyResult<bool> {
        let locals = [
            ("model", self.model.clone()),
            ("image_path", image_path.to_str().unwrap().into_py(self.py_interpreter)),
        ]
        .into_py_dict(self.py_interpreter);
        
        // Run prediction
        let result = self.py_interpreter.eval(
            "model(image_path, verbose=False)[0].boxes[0].cls.item()",
            None,
            Some(&locals),
        )?;
        
        // Convert prediction to bool (assuming binary classification)
        let class_idx: i64 = result.extract()?;
        Ok(class_idx == 1)
    }
}