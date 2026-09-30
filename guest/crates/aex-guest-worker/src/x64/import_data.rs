impl GuestEngine<'static> {
    // These ABI exports are objects, not functions. Their storage is private to
    // one guest engine and shared by every importing module in that engine.
    // Initial values: Microsoft STL stl/src/ios.cpp.
    fn resolve_emulated_import_data(
        &mut self,
        library: &str,
        symbol: &str,
    ) -> Result<Option<u64>, GuestError> {
        if normalize_import_library_name(library) != "msvcp140.dll" {
            return Ok(None);
        }
        let (key, bytes): (&'static str, &[u8]) = match symbol {
            "?_Index@ios_base@std@@0HA" => ("msvcp140.dll!ios_base::_Index", &[0; 4]),
            "?_Sync@ios_base@std@@0_NA" => ("msvcp140.dll!ios_base::_Sync", &[1]),
            _ => return Ok(None),
        };
        if let Some(address) = self.unicorn.get_data().imported_data.get(key) {
            return Ok(Some(*address));
        }
        let address = self.allocate(bytes.len(), 4)?;
        self.write(address, bytes)?;
        self.unicorn
            .get_data_mut()
            .imported_data
            .insert(key, address);
        Ok(Some(address))
    }

    fn link_emulated_import_data(&mut self, image: &PeImage) -> Result<(), GuestError> {
        for library in image.imports() {
            for symbol in &library.symbols {
                if let Some(address) =
                    self.resolve_emulated_import_data(&library.name, &symbol.name)?
                {
                    if symbol
                        .iat_rva
                        .checked_add(8)
                        .is_none_or(|end| end > image.mapped_bytes().len())
                    {
                        return Err(GuestError::IatRange);
                    }
                    self.write(
                        image.image_base() + symbol.iat_rva as u64,
                        &address.to_le_bytes(),
                    )?;
                }
            }
        }
        Ok(())
    }
}
