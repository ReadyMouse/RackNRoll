from ultralytics import YOLO
import cv2
import os
from pathlib import Path
import json
import argparse

class PoolTableInference():
    def __init__(self, model_path = './yolo_weights.pt',
                 output_dir = "outputs",
                 conf_threshold=0.5,
                 save=False,
                 save_negative=False
                 ):
        self.model_path = model_path
        self.conf_threshold = conf_threshold
        self.output_dir = output_dir if output_dir else "outputs" 
        self.save = save
        self.save_negative = save_negative

    def is_empty_dir(self, path):
        with os.scandir(path) as scan:
            return not any(scan)

    def run_inference(self, image_path, save_negative=None):
        """
        Run classification inference on a single image or directory of images from same venue
        """
        # Use instance save_negative if not explicitly provided
        save_negative = save_negative if save_negative is not None else self.save_negative
        
        # Load the model
        model = YOLO(self.model_path)
        print('Successfully loaded the model weights.')

        image_paths = []
        if os.path.isfile(image_path):
            print(f"Found file: {image_path}")  # Debug print
            image_paths = [image_path]
        else:
            print(f"Looking for images in directory: {image_path}") 
            image_paths = [str(p) for p in Path(image_path).glob('*')
                        if p.suffix.lower() in ['.jpg', '.jpeg', '.png']]
            print(f"Found images: {image_paths}")  # Debug print

        if not image_paths:
            print(f"Warning: No valid images found at {image_path}")
        
        all_results = {}
        highest_pool_table_conf = 0.0  # Track the highest confidence for pool table

        print(f"\nProcessing {image_paths}")
        for img_path in image_paths:
            # Run inference
            results = model.predict(
                source=img_path,
                conf=self.conf_threshold,
                save=False,   # Save the results
                project= os.path.dirname(self.output_dir),
                name=os.path.basename(self.output_dir),
                exist_ok=True, 
                verbose=False 
            )

            # Extract classification results
            result = {
                'class_name': results[0].names[results[0].probs.top1],  # Get class name
                'confidence': float(results[0].probs.top1conf),  # Get confidence
                'class_index': int(results[0].probs.top1)  # Get class index
            }

            # If this is a pool table detection, update highest confidence
            if result['class_name'] == 'pool_table':
                highest_pool_table_conf = max(highest_pool_table_conf, result['confidence'])
            
            # Remove photos without pool tables
            if not save_negative and result['class_name'] == 'no_pool_table':
                os.remove(os.path.join(self.output_dir, os.path.basename(img_path)))
            
            all_results[os.path.basename(img_path)] = result
            
            print(f"\nProcessed {img_path}")
            print(f"Prediction: {result['class_name']} ({result['confidence']:.2f} confidence)")
        
        # No photos of pool tables, remove the directory
        try:
            if not save_negative and self.is_empty_dir(self.output_dir): 
                os.remove(os.path.join(self.output_dir))
        except (PermissionError, OSError) as e:
            pass


        # Save results to JSON
        if self.save:
            results_file = os.path.join(self.output_dir, 'results.json')
            with open(results_file, 'w') as f:
                json.dump(all_results, f, indent=4)
        
        #print(f"\nResults saved to: {self.output_dir}")
        #print(f"- JSON results: {results_file}")

        return highest_pool_table_conf
       

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="A script that accepts command line arguments")
    
    # Add arguments
    parser.add_argument('--input_path','-i',
                       help='Path to the input image')
    parser.add_argument('--model_path','-m',
                       help='Path to the weight file')
    parser.add_argument('-o', '--save_path',
                       help='Path to save results')
    parser.add_argument('--save-negative',
                       type=lambda x: x.lower() == 'true',
                       default=False,
                       help='Whether to save images without pool tables')
    
    # Parse arguments
    args = parser.parse_args()
    
    # Use the parsed arguments
    engine = PoolTableInference(
        model_path=args.model_path,  
        output_dir=args.save_path,
        save_negative=args.save_negative
    )
    pool_table_probability = engine.run_inference(image_path=args.input_path) 
    print(f"VENUE_PROBABILITY:{pool_table_probability}") 