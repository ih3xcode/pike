//! Хендлери п'яти маршрутів. Кожен файл — один крок сценарію розгортання:
//! скрипт → колбек із фактами про хост → завантаження бінаря → звіт.

pub mod callback;
pub mod done;
pub mod download;
pub mod parse;
pub mod scripts;
