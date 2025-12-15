// Use absolute paths within the macro to avoid requiring imports at call sites.

#[macro_export]
macro_rules! fail {
    ($test:expr) => {
        ::hyperware_process_lib::Response::new()
            .body($crate::hyperware::process::tester::Response::Run(Err(
                $crate::hyperware::process::tester::FailResponse {
                test: $test.into(),
                file: file!().into(),
                line: line!(),
                column: column!(),
            },
            )))
            .send()
            .unwrap();
        panic!("")
    };
    ($test:expr, $file:expr, $line:expr, $column:expr) => {
        ::hyperware_process_lib::Response::new()
            .body($crate::hyperware::process::tester::Response::Run(Err(
                $crate::hyperware::process::tester::FailResponse {
                test: $test.into(),
                file: $file.into(),
                line: $line,
                column: $column,
            },
            )))
            .send()
            .unwrap();
        panic!("")
    };
}
