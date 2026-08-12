use std::vec;

use chrono::{Date, DateTime, Days, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use diesel::{Connection, SqliteConnection};
use egui::Response;
use egui_plot::{
    AxisHints, GridInput, GridMark, Legend, Line, Plot, PlotPoint, PlotPoints, Points, Polygon,
    Text,
};
use polars::{
    frame::DataFrame,
    prelude::{IntoLazy, col, lit},
};

use crate::panels::LatLon;
use crate::{
    models::ObjectPositionStored,
    solarsystemcalc::{OBJECT_NAMES_WITH_MOON, PLANET_NAMES, get_object_color},
};

pub struct LongTermPlot {
    pub dates: Vec<NaiveDate>,
    pub visibility_segments: DataFrame,
    pub sc: DataFrame,
    pub conn: SqliteConnection,
    lat_lon: LatLon,
}

impl LongTermPlot {
    pub fn new(lat_lon: LatLon) -> Self {
        Self {
            dates: vec![],
            visibility_segments: DataFrame::default(),
            sc: DataFrame::default(),
            conn: SqliteConnection::establish(
                &std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            )
            .unwrap_or_else(|_| {
                panic!(
                    "Error connecting to {}",
                    std::env::var("DATABASE_URL").expect("DATABASE_URL must be set")
                )
            }),
            lat_lon,
        }
    }
    pub fn refresh_from_db(&mut self) {
        // Load data from the database into self.visibility_segments and self.sc
        self.dates = ObjectPositionStored::available_days(&mut self.conn, &self.lat_lon.snap(2));
    }
}

impl egui::Widget for &mut LongTermPlot {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let time_formatter = |value: GridMark, _range: &std::ops::RangeInclusive<f64>| {
            let ndt = DateTime::<Utc>::from_timestamp_millis(value.value as i64).unwrap();
            ndt.format("%Y-%m-%d").to_string()
        };
        let hints = vec![AxisHints::new_x().label("Time").formatter(time_formatter)];
        let y_grid = |input: GridInput| {
            let mut marks = Vec::new();
            let start = input.bounds.0;
            let end = input.bounds.1;
            let start_dt = DateTime::<Utc>::from_timestamp_millis(start as i64).unwrap();
            let end_dt = DateTime::<Utc>::from_timestamp_millis(end as i64).unwrap();

            let date_dt = start_dt
                .with_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
                .unwrap();
            let mut current = date_dt.checked_add_days(Days::new(1)).unwrap(); // align to day start
            while current < end_dt {
                marks.push(GridMark {
                    value: current.timestamp_millis() as f64,
                    step_size: 86400000.0,
                });
                current = current.checked_add_days(Days::new(1)).unwrap(); // one day in milliseconds
            }
            marks
        };
        let mut plot = Plot::new("lines_demo")
            .show_axes(true)
            .show_grid(true)
            .custom_y_axes(hints)
            .y_grid_spacer(y_grid);

        ui.heading("Example");
        ui.horizontal(|ui| {
            ui.label("Name");
        });

        plot.show(ui, |plot_ui| {
            if self.visibility_segments.height() == 0 {
                return;
            }
        });
        ui.response()
    }
}
